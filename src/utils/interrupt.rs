use crate::error::{BiliLiveError, Result};
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

static INTERRUPTED: AtomicBool = AtomicBool::new(false);
static CTRLC_COUNT: AtomicUsize = AtomicUsize::new(0);
static ALT_SCREEN_ACTIVE: AtomicBool = AtomicBool::new(false);
static STDERR_CURSOR_HIDDEN: AtomicBool = AtomicBool::new(false);
static TEMP_PATHS: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

/// 是否已收到 Ctrl+C / SIGINT 中断信号
pub fn is_interrupted() -> bool {
    INTERRUPTED.load(Ordering::SeqCst)
}

/// 若已收到中断信号，立即返回 `Err(BiliLiveError::Interrupted)` 触发栈上 RAII 清理
pub fn check_interrupted() -> Result<()> {
    if is_interrupted() {
        Err(BiliLiveError::Interrupted)
    } else {
        Ok(())
    }
}

/// 可中断睡眠：按小时间片（<= 60ms）轮询 `is_interrupted()`，确保 Ctrl+C 在瞬间唤醒并清理栈帧
pub fn sleep_interruptible(total: Duration) -> Result<()> {
    check_interrupted()?;
    let start = Instant::now();
    while start.elapsed() < total {
        check_interrupted()?;
        let rem = total.saturating_sub(start.elapsed());
        if rem.is_zero() {
            break;
        }
        std::thread::sleep(rem.min(Duration::from_millis(60)));
    }
    check_interrupted()
}

/// 标记全屏备用屏幕缓冲区状态（供紧急中断兜底恢复）
pub fn set_alt_screen_active(active: bool) {
    ALT_SCREEN_ACTIVE.store(active, Ordering::SeqCst);
}

/// 标记 stderr 光标是否被隐藏（避免向重定向的 stdout 输出控制序列）
pub fn set_stderr_cursor_hidden(hidden: bool) {
    STDERR_CURSOR_HIDDEN.store(hidden, Ordering::SeqCst);
}

/// 注册需要兜底删除的临时文件路径
pub fn register_temp_path(path: PathBuf) {
    let mut guard = TEMP_PATHS.lock().unwrap_or_else(|e| e.into_inner());
    if !guard.iter().any(|p| p == &path) {
        guard.push(path);
    }
}

/// 取消注册临时文件路径
pub fn unregister_temp_path(path: &Path) {
    let mut guard = TEMP_PATHS.lock().unwrap_or_else(|e| e.into_inner());
    guard.retain(|p| p != path);
}

/// RAII 临时文件清理守卫：正常退栈自动删除文件并注销；极端强制退出时亦由信号兜底清理
pub struct TempFileCleanupGuard {
    path: PathBuf,
}

impl TempFileCleanupGuard {
    pub fn new(path: PathBuf) -> Self {
        register_temp_path(path.clone());
        Self { path }
    }
}

impl Drop for TempFileCleanupGuard {
    fn drop(&mut self) {
        unregister_temp_path(&self.path);
        let _ = std::fs::remove_file(&self.path);
    }
}

/// 清理所有已注册但尚未退栈的临时文件（仅用于阻塞系统调用超时兜底退出前）
pub fn cleanup_registered_temp_files() {
    let paths: Vec<PathBuf> = {
        let mut guard = TEMP_PATHS.lock().unwrap_or_else(|e| e.into_inner());
        std::mem::take(&mut *guard)
    };
    for p in paths {
        let _ = std::fs::remove_file(p);
    }
}

/// 恢复终端原始模式、备用屏幕与光标显示（严禁向非全屏重定向 stdout 写入 ANSI 序列）
pub fn restore_terminal_state() {
    let _ = crossterm::terminal::disable_raw_mode();

    if ALT_SCREEN_ACTIVE.swap(false, Ordering::SeqCst) && std::io::stdout().is_terminal() {
        let mut stdout = std::io::stdout();
        let _ = crossterm::execute!(
            stdout,
            crossterm::cursor::Show,
            crossterm::terminal::LeaveAlternateScreen
        );
    }

    if (STDERR_CURSOR_HIDDEN.swap(false, Ordering::SeqCst) || is_interrupted())
        && std::io::stderr().is_terminal()
    {
        let mut stderr = std::io::stderr();
        let _ = crossterm::execute!(stderr, crossterm::cursor::Show);
    }
}

/// 安装全局 Ctrl+C 处理器：
/// - 首次收到信号：置位 `INTERRUPTED`，让下载/扫码/倒计时/流监听循环在 <60ms 内优雅退栈触发 Drop；
///   若主线程阻塞在底层 DNS/Socket 调用超过 450ms 未自行退出，则由守护线程主动清理注册的临时文件并恢复终端后以 130 退出。
/// - 再次收到信号：立即执行兜底清理并以 130 退出。
pub fn install_ctrlc_handler() {
    let _ = ctrlc::set_handler(move || {
        let count = CTRLC_COUNT.fetch_add(1, Ordering::SeqCst) + 1;
        INTERRUPTED.store(true, Ordering::SeqCst);

        if count >= 2 {
            cleanup_registered_temp_files();
            restore_terminal_state();
            std::process::exit(130);
        }

        std::thread::spawn(|| {
            std::thread::sleep(Duration::from_millis(450));
            cleanup_registered_temp_files();
            restore_terminal_state();
            std::process::exit(130);
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn temp_file_cleanup_guard_removes_file_and_unregisters_on_drop() {
        let dir = tempfile::TempDir::new().unwrap();
        let file_path = dir.path().join("qrcode_temp_test.png");
        std::fs::write(&file_path, b"test").unwrap();
        assert!(file_path.exists());

        {
            let _guard = TempFileCleanupGuard::new(file_path.clone());
            let registered = TEMP_PATHS.lock().unwrap().contains(&file_path);
            assert!(registered);
        }

        assert!(!file_path.exists());
        let registered_after = TEMP_PATHS.lock().unwrap().contains(&file_path);
        assert!(!registered_after);
    }
}
