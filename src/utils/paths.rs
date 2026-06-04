use std::path::PathBuf;

/// 数据目录：Linux/macOS 为 ~/.config/bt，Windows 为 %APPDATA%/bt
///
/// macOS 需要特殊处理：
///   macOS 上 dirs::config_dir() 返回 ~/Library/Application Support
///   目标是 ~/.config/bt，所以手动拼接
///
///   Linux 上 dirs::config_dir() 遵循 XDG 规范直接返回 ~/.config，无需特殊处理
///   Windows 上 dirs::config_dir() 返回 %APPDATA%
pub fn data_dir() -> PathBuf {
    #[cfg(target_os = "macos")]
    {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        let path = home.join(".config").join("bt");
        std::fs::create_dir_all(&path).ok();
        return path;
    }
    #[cfg(not(target_os = "macos"))]
    {
        let path = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("bt");
        std::fs::create_dir_all(&path).ok();
        path
    }
}

/// 数据目录下的文件路径
pub fn data_file(filename: &str) -> PathBuf {
    data_dir().join(filename)
}
