use crate::cli::args::{Cli, Shell};
use crate::error::Result;
use crate::{user_info, user_success};
use clap::CommandFactory;
use clap_complete::{generate, shells};
use std::io::Write;

fn shell_name(shell: Shell) -> &'static str {
    match shell {
        Shell::Bash => "bash",
        Shell::Zsh => "zsh",
        Shell::Fish => "fish",
        Shell::PowerShell => "powershell",
        Shell::Elvish => "elvish",
    }
}

pub fn generate_completion_script(shell: Shell) -> Vec<u8> {
    let mut cmd = Cli::command();
    let mut buf = Vec::new();
    match shell {
        Shell::Bash => generate(shells::Bash, &mut cmd, "bt", &mut buf),
        Shell::Zsh => generate(shells::Zsh, &mut cmd, "bt", &mut buf),
        Shell::Fish => generate(shells::Fish, &mut cmd, "bt", &mut buf),
        Shell::PowerShell => generate(shells::PowerShell, &mut cmd, "bt", &mut buf),
        Shell::Elvish => generate(shells::Elvish, &mut cmd, "bt", &mut buf),
    }
    buf
}

fn detect_current_shell() -> Option<Shell> {
    let val = std::env::var("SHELL").ok()?;
    let lower = val.to_ascii_lowercase();
    if lower.ends_with("zsh") {
        Some(Shell::Zsh)
    } else if lower.ends_with("bash") {
        Some(Shell::Bash)
    } else if lower.ends_with("fish") {
        Some(Shell::Fish)
    } else if lower.ends_with("pwsh") || lower.ends_with("powershell") {
        Some(Shell::PowerShell)
    } else if lower.ends_with("elvish") {
        Some(Shell::Elvish)
    } else {
        None
    }
}

pub fn handle_completions(shell: Option<Shell>, install: bool) -> Result<()> {
    let shell = match shell.or_else(detect_current_shell) {
        Some(s) => s,
        None => {
            return Err(crate::error::BiliLiveError::Input(
                "未指定且无法从 $SHELL 自动识别 shell 类型，请显式指定：bt completions <zsh|bash|fish|powershell|elvish> [--install]"
                    .to_string(),
            ));
        }
    };
    let buf = generate_completion_script(shell);

    if install {
        let home = std::env::var("HOME")
            .ok()
            .and_then(|h| if h.is_empty() { None } else { Some(std::path::PathBuf::from(h)) })
            .or_else(dirs::home_dir)
            .unwrap_or_else(|| std::path::PathBuf::from("."));

        let (dir, file) = match shell {
            Shell::Bash => (
                home.join(".local/share/bash-completion/completions"),
                home.join(".local/share/bash-completion/completions/bt"),
            ),
            Shell::Zsh => (
                home.join(".zsh/completions"),
                home.join(".zsh/completions/_bt"),
            ),
            Shell::Fish => (
                home.join(".config/fish/completions"),
                home.join(".config/fish/completions/bt.fish"),
            ),
            Shell::PowerShell => (
                home.join(".config/powershell/completions"),
                home.join(".config/powershell/completions/bt.ps1"),
            ),
            Shell::Elvish => (
                home.join(".config/elvish/lib"),
                home.join(".config/elvish/lib/bt.elv"),
            ),
        };
        std::fs::create_dir_all(&dir)?;
        std::fs::write(&file, &buf)?;

        // 若检测到 Oh My Zsh，同步安装到 ~/.oh-my-zsh/custom/completions/_bt（默认已在 fpath 中，无需手改 .zshrc）
        let mut omz_installed = None;
        if matches!(shell, Shell::Zsh) {
            if home.join(".oh-my-zsh").is_dir() {
                let omz_dir = home.join(".oh-my-zsh/custom/completions");
                let omz_file = omz_dir.join("_bt");
                if std::fs::create_dir_all(&omz_dir).is_ok()
                    && std::fs::write(&omz_file, &buf).is_ok()
                {
                    omz_installed = Some(omz_file);
                }
            }
            // 清理旧的 ~/.zcompdump* 缓存，避免 ~/.zshrc 中使用 `compinit -C` 时跳过新安装的 _bt
            if let Ok(entries) = std::fs::read_dir(&home) {
                for entry in entries.flatten() {
                    if let Some(name) = entry.file_name().to_str()
                        && name.starts_with(".zcompdump")
                    {
                        let _ = std::fs::remove_file(entry.path());
                    }
                }
            }
        }

        if crate::cli::output::is_json() {
            crate::cli::output::print_json(&serde_json::json!({
                "installed": true,
                "shell": shell_name(shell),
                "path": file.display().to_string()
            }))?;
        } else {
            user_success!("补全脚本已安装到 {}", file.display());
            match shell {
                Shell::Zsh => {
                    if let Some(omz_path) = omz_installed {
                        user_success!("检测到 Oh My Zsh，已同步安装到 {}", omz_path.display());
                        user_info!("执行 `autoload -Uz compinit && compinit` 或重开终端即可直接按 Tab 补全");
                    } else {
                        user_info!("请在 ~/.zshrc 中添加 fpath=(~/.zsh/completions $fpath)，然后执行 exec zsh 生效");
                    }
                }
                Shell::Bash => user_info!("请执行 source {} 或重新打开终端生效", file.display()),
                Shell::Fish => user_info!("重新打开 fish 终端即可生效"),
                Shell::PowerShell => user_info!("请在 PowerShell $PROFILE 中添加 `. {}` 生效", file.display()),
                Shell::Elvish => user_info!("请在 ~/.config/elvish/rc.elv 中添加 `use bt` 生效"),
            }
        }
    } else if crate::cli::output::is_json() {
        crate::cli::output::print_json(&serde_json::json!({
            "shell": shell_name(shell),
            "script": String::from_utf8_lossy(&buf)
        }))?;
    } else {
        if let Err(e) = std::io::stdout().write_all(&buf)
            && e.kind() != std::io::ErrorKind::BrokenPipe
        {
            return Err(e.into());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generates_non_empty_scripts_for_all_supported_shells() {
        for shell in [
            Shell::Bash,
            Shell::Zsh,
            Shell::Fish,
            Shell::PowerShell,
            Shell::Elvish,
        ] {
            let bytes = generate_completion_script(shell);
            let script = String::from_utf8(bytes).unwrap();
            assert!(!script.trim().is_empty(), "{shell:?} script was empty");
            assert!(script.contains("bt"), "{shell:?} script missing 'bt'");
        }
    }
}
