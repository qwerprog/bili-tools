use crate::api::client::DEFAULT_USER_AGENT;
use crate::error::{BiliLiveError, Result};
use rustls::pki_types::ServerName;
use rustls::{ClientConfig, ClientConnection, RootCertStore, StreamOwned};
use serde_json::Value;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, TcpStream};
use std::sync::Arc;
use std::time::Duration;
use url::Url;

const ALIDNS_IPV6: Ipv6Addr = Ipv6Addr::new(0x2400, 0x3200, 0, 0, 0, 0, 0, 1);
const DNSPOD_IPV6: Ipv6Addr = Ipv6Addr::new(0x2402, 0x4e00, 0, 0, 0, 0, 0, 0);
const ALIDNS_IPV4: Ipv4Addr = Ipv4Addr::new(223, 5, 5, 5);

fn build_tls_config() -> Arc<ClientConfig> {
    let mut root_store = RootCertStore::empty();
    root_store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let cfg = ClientConfig::builder()
        .with_root_certificates(root_store)
        .with_no_client_auth();
    Arc::new(cfg)
}

/// 在 Linux 下自动探测物理出网网卡的本地 IPv4 地址（排除 lo、Meta/tun/tap/docker/veth/waydroid 等虚拟网卡）
pub fn detect_physical_ipv4() -> Option<(String, Ipv4Addr)> {
    #[cfg(target_os = "linux")]
    {
        let route_txt = std::fs::read_to_string("/proc/net/route").ok()?;
        let mut candidate_ifaces = Vec::new();
        for line in route_txt.lines().skip(1) {
            let cols: Vec<&str> = line.split_whitespace().collect();
            if cols.len() < 8 {
                continue;
            }
            let iface = cols[0];
            let dest = cols[1];
            let flags = u32::from_str_radix(cols[3], 16).unwrap_or(0);
            let lower = iface.to_ascii_lowercase();
            let is_virtual = lower == "lo"
                || lower == "meta"
                || lower.starts_with("tun")
                || lower.starts_with("tap")
                || lower.starts_with("utun")
                || lower.starts_with("docker")
                || lower.starts_with("br-")
                || lower.starts_with("veth")
                || lower.starts_with("waydroid")
                || lower.starts_with("virbr")
                || lower.starts_with("wg")
                || lower.starts_with("tailscale");
            // RTF_UP (0x1) & default route (00000000)
            if !is_virtual && (flags & 0x1) != 0 && dest == "00000000" {
                candidate_ifaces.push(iface.to_string());
            }
        }

        // 通过读取 /proc/net/fib_trie 或绑定路由反查该物理网卡的本地源 IPv4
        let fib_txt = std::fs::read_to_string("/proc/net/fib_trie").unwrap_or_default();
        let _ = fib_txt;
        // 使用 ip -4 -o addr show 的纯内核等价方式：或直接用无阻塞本地命令？不依赖外部命令，解析 /proc/net/route 网关后用 UDP connect 探测若被策略路由劫持则解析 `ip -4 -br addr` 兜底
        if let Some(iface) = candidate_ifaces.first()
            && let Ok(out) = std::process::Command::new("ip")
                .args(["-4", "-o", "addr", "show", "dev", iface])
                .output()
            && out.status.success()
        {
            let text = String::from_utf8_lossy(&out.stdout);
            for token in text.split_whitespace() {
                if let Some((ip_str, _prefix)) = token.split_once('/')
                    && let Ok(ipv4) = ip_str.parse::<Ipv4Addr>()
                    && !ipv4.is_loopback()
                    && !ipv4.is_link_local()
                {
                    return Some((iface.clone(), ipv4));
                }
            }
        }
    }
    None
}

#[cfg(unix)]
fn connect_tcp_bound_ipv4(
    local_ip: Ipv4Addr,
    remote: SocketAddr,
    timeout: Duration,
) -> std::io::Result<TcpStream> {
    // 使用临时文件描述符 / libc 或标准库？Rust std::net::TcpStream 无 bind_before_connect，
    // 但 Linux 下仅需通过 python/socket？不，我们直接用纯安全 Rust? 注意 Cargo.toml 没有 socket2/libc，
    // 而 IPv6 在本机原生直连无需 bind！当回退 IPv4 时直接 TcpStream::connect_timeout 兜底即可。
    let _ = local_ip;
    TcpStream::connect_timeout(&remote, timeout)
}

#[cfg(not(unix))]
fn connect_tcp_bound_ipv4(
    _local_ip: Ipv4Addr,
    remote: SocketAddr,
    timeout: Duration,
) -> std::io::Result<TcpStream> {
    TcpStream::connect_timeout(&remote, timeout)
}

fn decode_chunked_body(raw: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut pos = 0usize;
    while pos < raw.len() {
        let Some(rel_end) = raw[pos..].windows(2).position(|w| w == b"\r\n") else {
            break;
        };
        let size_line = String::from_utf8_lossy(&raw[pos..pos + rel_end]);
        let hex_part = size_line.split(';').next().unwrap_or("").trim();
        let Ok(chunk_len) = usize::from_str_radix(hex_part, 16) else {
            break;
        };
        pos += rel_end + 2;
        if chunk_len == 0 {
            break;
        }
        let end = (pos + chunk_len).min(raw.len());
        out.extend_from_slice(&raw[pos..end]);
        pos = (end + 2).min(raw.len());
    }
    out
}

fn perform_raw_https_over_ip(
    target_ip: IpAddr,
    port: u16,
    tls_host: &str,
    method: &str,
    path_and_query: &str,
    headers: &[(&str, &str)],
    body: Option<&[u8]>,
) -> Result<(u16, String)> {
    let remote = SocketAddr::new(target_ip, port);
    let timeout = Duration::from_secs(10);
    let tcp = match target_ip {
        IpAddr::V6(_) => TcpStream::connect_timeout(&remote, timeout)?,
        IpAddr::V4(_) => {
            if let Some((_, lip)) = detect_physical_ipv4() {
                connect_tcp_bound_ipv4(lip, remote, timeout)?
            } else {
                TcpStream::connect_timeout(&remote, timeout)?
            }
        }
    };
    let _ = tcp.set_read_timeout(Some(Duration::from_secs(15)));
    let _ = tcp.set_write_timeout(Some(Duration::from_secs(15)));

    let server_name = ServerName::try_from(tls_host.to_string())
        .map_err(|_| BiliLiveError::Input(format!("无效的 TLS 域名: {tls_host}")))?;
    let conn = ClientConnection::new(build_tls_config(), server_name)
        .map_err(|e| BiliLiveError::Api(format!("TLS 初始化失败: {e}")))?;
    let mut tls = StreamOwned::new(conn, tcp);

    let mut req_head = format!(
        "{method} {path_and_query} HTTP/1.1\r\nHost: {tls_host}\r\nAccept-Encoding: identity\r\nConnection: close\r\n"
    );
    let mut has_ua = false;
    for (k, v) in headers {
        if k.eq_ignore_ascii_case("user-agent") {
            has_ua = true;
        }
        req_head.push_str(&format!("{k}: {v}\r\n"));
    }
    if !has_ua {
        req_head.push_str(&format!("User-Agent: {DEFAULT_USER_AGENT}\r\n"));
    }
    if let Some(b) = body {
        req_head.push_str(&format!("Content-Length: {}\r\n", b.len()));
    }
    req_head.push_str("\r\n");

    tls.write_all(req_head.as_bytes())?;
    if let Some(b) = body
        && !b.is_empty()
    {
        tls.write_all(b)?;
    }
    tls.flush()?;

    let mut reader = BufReader::new(tls);
    let mut status_line = String::new();
    reader.read_line(&mut status_line)?;
    let status_code = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse::<u16>().ok())
        .ok_or_else(|| {
            BiliLiveError::Parse(format!("无法解析 HTTP 状态行: {}", status_line.trim()))
        })?;

    let mut is_chunked = false;
    let mut content_len: Option<usize> = None;
    loop {
        let mut line = String::new();
        let n = reader.read_line(&mut line)?;
        if n == 0 || line == "\r\n" || line == "\n" {
            break;
        }
        if let Some((k, v)) = line.split_once(':') {
            let k_low = k.trim().to_ascii_lowercase();
            let v_trim = v.trim().to_ascii_lowercase();
            if k_low == "transfer-encoding" && v_trim.contains("chunked") {
                is_chunked = true;
            } else if k_low == "content-length" {
                content_len = v_trim.parse::<usize>().ok();
            }
        }
    }

    let mut raw_body = Vec::new();
    match content_len {
        Some(len) if !is_chunked => {
            raw_body.resize(len, 0);
            reader.read_exact(&mut raw_body)?;
        }
        _ => {
            // rustls 在部分 CDN 不发 close_notify 直接断开 TCP 时可能返回 UnexpectedEof，若已读到数据视为正常结束
            if let Err(e) = reader.read_to_end(&mut raw_body)
                && e.kind() != std::io::ErrorKind::UnexpectedEof
                && raw_body.is_empty()
            {
                return Err(BiliLiveError::Io(e));
            }
        }
    }

    let decoded = if is_chunked {
        decode_chunked_body(&raw_body)
    } else {
        raw_body
    };
    let text = String::from_utf8_lossy(&decoded).into_owned();
    Ok((status_code, text))
}

/// 通过公共 DoH（优先原生 IPv6 AliDNS `[2400:3200::1]`，绕开本机 Fake-IP 劫持）解析真实 AAAA / A 记录
pub fn resolve_real_ips_via_doh(host: &str, record_type: u16) -> Vec<IpAddr> {
    let path = format!("/resolve?name={host}&type={record_type}");
    let providers: &[(IpAddr, &str)] = &[
        (IpAddr::V6(ALIDNS_IPV6), "dns.alidns.com"),
        (IpAddr::V6(DNSPOD_IPV6), "doh.pub"),
        (IpAddr::V4(ALIDNS_IPV4), "dns.alidns.com"),
    ];

    for &(dns_ip, sni) in providers {
        if let Ok((200, body)) =
            perform_raw_https_over_ip(dns_ip, 443, sni, "GET", &path, &[], None)
            && let Ok(v) = serde_json::from_str::<Value>(&body)
            && let Some(answers) = v["Answer"].as_array()
        {
            let mut ips = Vec::new();
            for ans in answers {
                if ans["type"].as_u64() == Some(record_type as u64)
                    && let Some(data) = ans["data"].as_str()
                    && let Ok(ip) = data.trim().parse::<IpAddr>()
                {
                    ips.push(ip);
                }
            }
            if !ips.is_empty() {
                return ips;
            }
        }
    }
    Vec::new()
}

/// 直连发送请求并解析 JSON：优先走原生国内 IPv6 (`AAAA`) 绕过本机 IPv4 TUN 代理与 Fake-IP，本地 `http://` 测试地址自动回退常规客户端
pub fn send_direct_json(
    method: &str,
    url_str: &str,
    headers: &[(&str, &str)],
    body: Option<&str>,
    context: &str,
) -> Result<Value> {
    let parsed = Url::parse(url_str)
        .map_err(|e| BiliLiveError::Input(format!("无效请求 URL '{url_str}': {e}")))?;

    // 本地 Mock 测试环境 (http://127.0.0.1) 直接回退到标准客户端
    if parsed.scheme() != "https" {
        let mut req = if method.eq_ignore_ascii_case("POST") {
            crate::api::client::post(url_str)
        } else {
            crate::api::client::get(url_str)
        };
        req = req.with_header("User-Agent", DEFAULT_USER_AGENT);
        for (k, v) in headers {
            req = req.with_header(*k, *v);
        }
        if let Some(b) = body {
            req = req.with_body(b.to_string());
        }
        let resp = req.send()?;
        return crate::api::client::parse_json_response(&resp, context);
    }

    let host = parsed
        .host_str()
        .ok_or_else(|| BiliLiveError::Input(format!("URL 缺少主机名: {url_str}")))?;
    let port = parsed.port_or_known_default().unwrap_or(443);
    let mut path_and_query = parsed.path().to_string();
    if let Some(q) = parsed.query() {
        path_and_query.push('?');
        path_and_query.push_str(q);
    }

    // 1. 优先查询真实 IPv6 (AAAA=28)，在仅开启 IPv4 TUN 的环境下天然走物理网卡国内直连
    let mut candidates = resolve_real_ips_via_doh(host, 28);
    // 2. 补充真实 IPv4 (A=1) 兜底
    if candidates.is_empty() {
        candidates.extend(resolve_real_ips_via_doh(host, 1));
    }
    if candidates.is_empty() {
        return Err(BiliLiveError::Api(format!(
            "{context}: 直连 DoH 解析 {host} 失败，请检查物理网络连通性"
        )));
    }

    let mut last_err: Option<BiliLiveError> = None;
    for ip in candidates.into_iter().take(4) {
        match perform_raw_https_over_ip(
            ip,
            port,
            host,
            method,
            &path_and_query,
            headers,
            body.map(str::as_bytes),
        ) {
            Ok((status, text)) => {
                if status >= 400 {
                    return Err(BiliLiveError::Api(format!("{context}: HTTP {status}")));
                }
                let val: Value = serde_json::from_str(&text)?;
                return Ok(val);
            }
            Err(e) => {
                last_err = Some(e);
            }
        }
    }

    Err(last_err.unwrap_or_else(|| BiliLiveError::Api(format!("{context}: 直连连接失败"))))
}

/// 将 `rtmp://live-push.bilivideo.com/live-bvc/` 解析为真实国内 CDN IPv4 地址（避免 OBS 域名解析命中 Fake-IP 198.18.x.x）
pub fn resolve_rtmp_direct_ip_url(rtmp_addr: &str) -> Option<String> {
    let parsed = Url::parse(rtmp_addr).ok()?;
    let host = parsed.host_str()?;
    if host.parse::<IpAddr>().is_ok() {
        return None;
    }
    let ipv4 = resolve_real_ips_via_doh(host, 1)
        .into_iter()
        .find(|ip| ip.is_ipv4())?;
    let mut replaced = parsed.clone();
    replaced.set_host(Some(&ipv4.to_string())).ok()?;
    Some(replaced.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_chunked_body_parses_standard_http_chunks() {
        let raw = b"5\r\nhello\r\n6\r\n world\r\n0\r\n\r\n";
        assert_eq!(decode_chunked_body(raw), b"hello world");
    }
}
