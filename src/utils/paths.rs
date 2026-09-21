use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// Linux 遵循 XDG，macOS 使用 ~/.config/bt，Windows 使用 %APPDATA%/bt。
pub fn data_dir() -> io::Result<PathBuf> {
    #[cfg(target_os = "macos")]
    let base = dirs::home_dir().map(|home| home.join(".config"));
    #[cfg(not(target_os = "macos"))]
    let base = dirs::config_dir();
    let path = base
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "无法确定配置目录"))?
        .join("bt");
    prepare_data_dir(&path)?;
    Ok(path)
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
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "文件缺少父目录"))?;
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
}
