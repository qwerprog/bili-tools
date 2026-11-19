use crate::error::Result;
use std::path::PathBuf;
use std::sync::RwLock;
use std::sync::atomic::{AtomicBool, Ordering};

static JSON_MODE: AtomicBool = AtomicBool::new(false);
static QUIET: AtomicBool = AtomicBool::new(false);
static YES_MODE: AtomicBool = AtomicBool::new(false);
static NO_COLOR_MODE: AtomicBool = AtomicBool::new(false);
static DIRECT_MODE: AtomicBool = AtomicBool::new(false);
static COOKIE_FILE: RwLock<Option<PathBuf>> = RwLock::new(None);

/// 初始化全局输出模式配置
pub fn init_output(
    json: bool,
    quiet: bool,
    yes: bool,
    no_color: bool,
    cookie_file: Option<PathBuf>,
) {
    JSON_MODE.store(json, Ordering::Relaxed);
    QUIET.store(quiet, Ordering::Relaxed);
    YES_MODE.store(yes, Ordering::Relaxed);
    NO_COLOR_MODE.store(no_color, Ordering::Relaxed);
    let mut guard = COOKIE_FILE.write().unwrap_or_else(|e| e.into_inner());
    *guard = cookie_file;
}

pub fn set_no_color(no_color: bool) {
    NO_COLOR_MODE.store(no_color, Ordering::Relaxed);
}

pub fn set_direct(direct: bool) {
    DIRECT_MODE.store(direct, Ordering::Relaxed);
}

/// 是否开启了 -D / --direct 国内直连模式
pub fn is_direct() -> bool {
    DIRECT_MODE.load(Ordering::Relaxed)
}

/// 是否开启了 --json 输出模式
pub fn is_json() -> bool {
    JSON_MODE.load(Ordering::Relaxed)
}

/// 是否开启了 --quiet / -q 静默模式
pub fn is_quiet() -> bool {
    QUIET.load(Ordering::Relaxed)
}

/// 是否开启了 --yes / -y 自动确认模式
pub fn is_yes() -> bool {
    YES_MODE.load(Ordering::Relaxed)
}

/// 是否开启了 --no-color 禁用彩色输出模式
pub fn is_no_color() -> bool {
    NO_COLOR_MODE.load(Ordering::Relaxed)
}

/// 获取全局指定的自定义 Cookie 文件路径
#[allow(dead_code)]
pub fn cookie_file() -> Option<PathBuf> {
    COOKIE_FILE.read().unwrap_or_else(|e| e.into_inner()).clone()
}

fn write_spacer_to<W: std::io::Write>(writer: &mut W) -> std::io::Result<()> {
    match writeln!(writer) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => Ok(()),
        Err(e) => Err(e),
    }
}

/// 打印格式化空行分隔符（静默模式与 JSON 模式下不输出）
pub fn print_spacer() {
    if !is_quiet() && !is_json() {
        let mut stdout = std::io::stdout().lock();
        let _ = write_spacer_to(&mut stdout);
    }
}

/// 在交互式 TTY 下向用户发起二次确认；支持通过 `stdin_is_tty` 注入终端状态以供测试。
/// - `-y/--yes` 时直接返回 `Ok(true)`
/// - `--json` 或非 TTY 且未传 `-y` 时返回 `BiliLiveError::Input`
pub fn confirm_action_with_tty(prompt: &str, stdin_is_tty: bool) -> Result<bool> {
    if is_yes() {
        return Ok(true);
    }
    if is_json() || !stdin_is_tty {
        return Err(crate::error::BiliLiveError::Input(
            "Confirmation required in non-interactive mode; pass --yes".to_string(),
        ));
    }
    dialoguer::Confirm::with_theme(&crate::ui::theme::bili_dialoguer_theme())
        .with_prompt(prompt)
        .default(false)
        .interact()
        .map_err(|e| crate::error::BiliLiveError::Input(format!("读取确认输入失败: {e}")))
}

/// 在需要确认的破坏性操作前向用户确认
pub fn confirm_action(prompt: &str) -> Result<bool> {
    use std::io::IsTerminal;
    confirm_action_with_tty(prompt, std::io::stdin().is_terminal())
}

pub fn json_error_value(kind: &str, message: &str) -> serde_json::Value {
    serde_json::json!({
        "ok": false,
        "error": {
            "kind": kind,
            "message": message,
        }
    })
}

/// 以标准格式向 stdout 打印 JSON 结果
pub fn print_json<T: serde::Serialize>(value: &T) -> Result<()> {
    use std::io::Write;
    let s = serde_json::to_string_pretty(value)?;
    let mut stdout = std::io::stdout().lock();
    if let Err(e) = writeln!(stdout, "{}", s)
        && e.kind() != std::io::ErrorKind::BrokenPipe
    {
        return Err(e.into());
    }
    Ok(())
}

/// 以标准错误信封向 stdout 打印 JSON 错误
pub fn print_json_error(kind: &str, message: &str) {
    use std::io::Write;
    let payload = json_error_value(kind, message);
    let mut stdout = std::io::stdout().lock();
    if let Ok(s) = serde_json::to_string_pretty(&payload) {
        let _ = writeln!(stdout, "{}", s);
    } else {
        let _ = writeln!(
            stdout,
            r#"{{"ok":false,"error":{{"kind":"Internal","message":"JSON serialization failed"}}}}"#
        );
    }
}

#[cfg(test)]
pub(crate) static TEST_OUTPUT_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
mod tests {
    use super::*;

    struct ResetOutputGuard;
    impl Drop for ResetOutputGuard {
        fn drop(&mut self) {
            init_output(false, false, false, false, None);
        }
    }

    #[test]
    fn output_state_round_trip_and_json_error_envelope() {
        let _lock = TEST_OUTPUT_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let _reset = ResetOutputGuard;

        init_output(
            true,
            true,
            true,
            true,
            Some(PathBuf::from("/tmp/bt_test_cookies.json")),
        );
        assert!(is_json());
        assert!(is_quiet());
        assert!(is_yes());
        assert!(is_no_color());
        assert_eq!(
            cookie_file(),
            Some(PathBuf::from("/tmp/bt_test_cookies.json"))
        );

        set_no_color(false);
        assert!(!is_no_color());

        let err_json = json_error_value("Input", "bad arg");
        assert_eq!(err_json["ok"], false);
        assert_eq!(err_json["error"]["kind"], "Input");
        assert_eq!(err_json["error"]["message"], "bad arg");
    }

    #[test]
    fn confirm_action_enforces_yes_and_non_interactive_policy() {
        let _lock = TEST_OUTPUT_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let _reset = ResetOutputGuard;

        // non-TTY without --yes -> Input error
        init_output(false, false, false, false, None);
        let err = confirm_action_with_tty("confirm?", false).unwrap_err();
        assert!(matches!(err, crate::error::BiliLiveError::Input(_)));
        assert!(
            err.to_string()
                .contains("Confirmation required in non-interactive mode; pass --yes")
        );

        // --json even on TTY without --yes -> Input error
        init_output(true, false, false, false, None);
        let err = confirm_action_with_tty("confirm?", true).unwrap_err();
        assert!(matches!(err, crate::error::BiliLiveError::Input(_)));

        // --yes on non-TTY -> Ok(true)
        init_output(false, false, true, false, None);
        assert!(confirm_action_with_tty("confirm?", false).unwrap());

        // --yes with --json on non-TTY -> Ok(true)
        init_output(true, false, true, false, None);
        assert!(confirm_action_with_tty("confirm?", false).unwrap());
    }

    struct BrokenPipeWriter;
    impl std::io::Write for BrokenPipeWriter {
        fn write(&mut self, _buf: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::from(std::io::ErrorKind::BrokenPipe))
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    struct OtherErrWriter;
    impl std::io::Write for OtherErrWriter {
        fn write(&mut self, _buf: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied))
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn write_spacer_ignores_broken_pipe_and_writes_newline_normally() {
        let mut buf = Vec::new();
        write_spacer_to(&mut buf).expect("normal write should succeed");
        assert_eq!(buf, b"\n");

        let mut broken = BrokenPipeWriter;
        write_spacer_to(&mut broken).expect("BrokenPipe should be ignored cleanly");

        let mut other = OtherErrWriter;
        let err = write_spacer_to(&mut other).expect_err("non-BrokenPipe error should propagate");
        assert_eq!(err.kind(), std::io::ErrorKind::PermissionDenied);
    }
}
