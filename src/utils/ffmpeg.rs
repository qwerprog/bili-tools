use crate::error::{BiliLiveError, Result};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Command;

#[cfg(windows)]
const FFMPEG_BIN: &str = "ffmpeg.exe";
#[cfg(not(windows))]
const FFMPEG_BIN: &str = "ffmpeg";

fn is_executable_file(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path)
            .map(|m| (m.permissions().mode() & 0o111) != 0)
            .unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        true
    }
}

pub fn missing_ffmpeg_message() -> String {
    "未找到可用的 ffmpeg 可执行文件。\n\
请安装 ffmpeg 并确保其位于 PATH 中，或通过环境变量 BT_FFMPEG_PATH 指定 ffmpeg 路径：\n\
  - Ubuntu/Debian: sudo apt install ffmpeg\n\
  - Fedora/RHEL:   sudo dnf install ffmpeg\n\
  - Arch Linux:    sudo pacman -S ffmpeg\n\
  - macOS:         brew install ffmpeg\n\
  - Windows:       winget install Gyan.FFmpeg  或  scoop install ffmpeg"
        .to_string()
}

pub fn find_ffmpeg_in_path(
    override_path: Option<&OsStr>,
    path_env: Option<&OsStr>,
) -> Option<PathBuf> {
    if let Some(raw) = override_path
        && !raw.is_empty()
    {
        let p = PathBuf::from(raw);
        if p.components().count() > 1 {
            return is_executable_file(&p).then_some(p);
        }
        if let Some(paths) = path_env {
            for dir in std::env::split_paths(paths) {
                let candidate = dir.join(&p);
                if is_executable_file(&candidate) {
                    return Some(candidate);
                }
            }
        }
        return None;
    }

    let paths = path_env?;
    for dir in std::env::split_paths(paths) {
        let candidate = dir.join(FFMPEG_BIN);
        if is_executable_file(&candidate) {
            return Some(candidate);
        }
    }
    None
}

#[allow(dead_code)]
pub fn find_ffmpeg() -> Option<PathBuf> {
    let override_path = std::env::var_os("BT_FFMPEG_PATH");
    let path_env = std::env::var_os("PATH");
    find_ffmpeg_in_path(override_path.as_deref(), path_env.as_deref())
}

pub fn ensure_ffmpeg_with(
    override_path: Option<&OsStr>,
    path_env: Option<&OsStr>,
) -> Result<PathBuf> {
    let path = find_ffmpeg_in_path(override_path, path_env)
        .ok_or_else(|| BiliLiveError::ExternalTool(missing_ffmpeg_message()))?;

    let output = Command::new(&path).arg("-version").output().map_err(|e| {
        BiliLiveError::ExternalTool(format!(
            "无法执行 ffmpeg ({}): {e}\n{}",
            path.display(),
            missing_ffmpeg_message()
        ))
    })?;

    if !output.status.success() {
        return Err(BiliLiveError::ExternalTool(format!(
            "ffmpeg ({}) 执行失败 (退出码 {:?})\n{}",
            path.display(),
            output.status.code(),
            missing_ffmpeg_message()
        )));
    }

    Ok(path)
}

#[allow(dead_code)]
pub fn ensure_ffmpeg() -> Result<PathBuf> {
    let override_path = std::env::var_os("BT_FFMPEG_PATH");
    let path_env = std::env::var_os("PATH");
    ensure_ffmpeg_with(override_path.as_deref(), path_env.as_deref())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;

    #[test]
    fn missing_path_returns_none() {
        assert_eq!(find_ffmpeg_in_path(None, None), None);
        assert_eq!(find_ffmpeg_in_path(None, Some(OsStr::new(""))), None);
        let empty_dir = tempfile::tempdir().unwrap();
        assert_eq!(
            find_ffmpeg_in_path(None, Some(empty_dir.path().as_os_str())),
            None
        );
    }

    #[cfg(unix)]
    #[test]
    fn finds_executable_in_path_and_ignores_non_executable_or_dir() {
        use std::os::unix::fs::PermissionsExt;

        let dir_non_exec = tempfile::tempdir().unwrap();
        let non_exec_file = dir_non_exec.path().join(FFMPEG_BIN);
        std::fs::write(&non_exec_file, b"#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&non_exec_file, std::fs::Permissions::from_mode(0o644)).unwrap();

        let dir_subdir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir_subdir.path().join(FFMPEG_BIN)).unwrap();

        let dir_exec = tempfile::tempdir().unwrap();
        let exec_file = dir_exec.path().join(FFMPEG_BIN);
        std::fs::write(&exec_file, b"#!/bin/sh\necho ffmpeg version 6.0\n").unwrap();
        std::fs::set_permissions(&exec_file, std::fs::Permissions::from_mode(0o755)).unwrap();

        let combined: OsString = std::env::join_paths([
            dir_non_exec.path(),
            dir_subdir.path(),
            dir_exec.path(),
        ])
        .unwrap();

        assert_eq!(
            find_ffmpeg_in_path(None, Some(&combined)),
            Some(exec_file.clone())
        );
        assert_eq!(ensure_ffmpeg_with(None, Some(&combined)).unwrap(), exec_file);
    }

    #[cfg(unix)]
    #[test]
    fn override_missing_path_does_not_fallback() {
        use std::os::unix::fs::PermissionsExt;

        let dir_exec = tempfile::tempdir().unwrap();
        let exec_file = dir_exec.path().join(FFMPEG_BIN);
        std::fs::write(&exec_file, b"#!/bin/sh\necho ok\n").unwrap();
        std::fs::set_permissions(&exec_file, std::fs::Permissions::from_mode(0o755)).unwrap();

        let nonexistent = dir_exec.path().join("missing/ffmpeg");
        assert_eq!(
            find_ffmpeg_in_path(
                Some(nonexistent.as_os_str()),
                Some(dir_exec.path().as_os_str())
            ),
            None
        );
    }

    #[test]
    fn ensure_ffmpeg_with_missing_returns_actionable_external_tool_error() {
        let empty_dir = tempfile::tempdir().unwrap();
        let err = ensure_ffmpeg_with(None, Some(empty_dir.path().as_os_str())).unwrap_err();
        assert!(matches!(err, BiliLiveError::ExternalTool(_)));
        let msg = err.to_string();
        assert!(msg.contains("ffmpeg"));
        assert!(msg.contains("PATH"));
        assert!(msg.contains("apt") || msg.contains("brew") || msg.contains("winget"));
    }
}
