use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::RwLock;

static COOKIE_FILE_OVERRIDE: RwLock<Option<PathBuf>> = RwLock::new(None);

pub fn set_cookie_file_override(path: Option<PathBuf>) {
    let mut guard = COOKIE_FILE_OVERRIDE
        .write()
        .unwrap_or_else(|e| e.into_inner());
    *guard = path;
}

pub fn cookies_path() -> io::Result<PathBuf> {
    if let Some(custom) = COOKIE_FILE_OVERRIDE
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
    {
        return Ok(custom);
    }
    data_file("cookies.json")
}

/// Linux 遵循 XDG，macOS 使用 ~/.config/bit，Windows 使用 %APPDATA%/bit（并兼容迁移旧 bt 目录）。
pub fn data_dir() -> io::Result<PathBuf> {
    #[cfg(target_os = "macos")]
    let base = dirs::home_dir().map(|home| home.join(".config"));
    #[cfg(target_os = "windows")]
    let base = std::env::var_os("APPDATA")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(dirs::config_dir);
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let base = dirs::config_dir();
    let root = base.ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "无法确定配置目录"))?;
    let path = root.join("bit");
    prepare_data_dir(&path)?;
    migrate_legacy_bt_dir(&root.join("bt"), &path);
    Ok(path)
}

fn migrate_legacy_bt_dir(legacy: &Path, target: &Path) {
    if !legacy.is_dir() || target.join("cookies.json").exists() {
        return;
    }
    for name in ["cookies.json", "stream_info.txt"] {
        let src = legacy.join(name);
        let dst = target.join(name);
        if src.is_file()
            && !dst.exists()
            && let Ok(bytes) = fs::read(&src)
        {
            let _ = write_private(&dst, &bytes);
        }
    }
}

fn prepare_data_dir(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(path)?;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
        // 同时收紧旧版本生成的文件权限，不读取凭据内容。
        for name in ["cookies.json", "stream_info.txt", "qrcode.png"] {
            match fs::set_permissions(path.join(name), fs::Permissions::from_mode(0o600)) {
                Ok(()) => {}
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                Err(e) => return Err(e),
            }
        }
    }
    #[cfg(not(unix))]
    fs::create_dir_all(path)?;
    Ok(())
}

pub fn data_file(filename: &str) -> io::Result<PathBuf> {
    Ok(data_dir()?.join(filename))
}

/// 临时文件在 Unix 上以 0600 创建，完整写入后替换目标，避免截断旧凭据。
pub fn write_private(path: &Path, contents: &[u8]) -> io::Result<()> {
    let raw_parent = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "文件缺少父目录"))?;
    let parent = if raw_parent.as_os_str().is_empty() {
        Path::new(".")
    } else {
        raw_parent
    };
    if !parent.exists() {
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(parent)?;
        }
        #[cfg(not(unix))]
        fs::create_dir_all(parent)?;
    }
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    file.write_all(contents)?;
    file.as_file().sync_all()?;
    file.persist(path).map_err(|e| e.error)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_write_replaces_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cookies.json");
        write_private(&path, b"long old contents").unwrap();
        write_private(&path, b"new").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"new");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn tightens_legacy_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o755)).unwrap();
        for name in ["cookies.json", "stream_info.txt"] {
            let path = dir.path().join(name);
            fs::write(&path, b"test").unwrap();
            fs::set_permissions(path, fs::Permissions::from_mode(0o644)).unwrap();
        }
        prepare_data_dir(dir.path()).unwrap();
        assert_eq!(
            fs::metadata(dir.path()).unwrap().permissions().mode() & 0o777,
            0o700
        );
        for name in ["cookies.json", "stream_info.txt"] {
            assert_eq!(
                fs::metadata(dir.path().join(name))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
    }

    #[test]
    fn write_private_creates_missing_parent_directory() {
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join("sub_a").join("sub_b").join("cookies.json");
        write_private(&nested, b"{}").unwrap();
        assert_eq!(fs::read(&nested).unwrap(), b"{}");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&nested).unwrap().permissions().mode() & 0o777,
                0o600
            );
            assert_eq!(
                fs::metadata(dir.path().join("sub_a").join("sub_b"))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o700
            );
        }
    }

    #[test]
    fn cookie_file_override_round_trip() {
        let _lock = crate::cli::output::TEST_OUTPUT_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        struct ResetOverride;
        impl Drop for ResetOverride {
            fn drop(&mut self) {
                set_cookie_file_override(None);
            }
        }
        let _reset = ResetOverride;

        let custom = PathBuf::from("/tmp/bt_custom_cookie_test.json");
        set_cookie_file_override(Some(custom.clone()));
        assert_eq!(cookies_path().unwrap(), custom);
    }
}
