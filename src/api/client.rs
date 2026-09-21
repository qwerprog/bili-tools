pub const DEFAULT_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.0.0 Safari/537.36 Edg/130.0.0.0";

const REQUEST_TIMEOUT_SECS: u64 = 30;

pub fn get(url: impl AsRef<str>) -> minreq::Request {
    minreq::get(url.as_ref()).with_timeout(REQUEST_TIMEOUT_SECS)
}

pub fn post(url: impl AsRef<str>) -> minreq::Request {
    minreq::post(url.as_ref()).with_timeout(REQUEST_TIMEOUT_SECS)
}
