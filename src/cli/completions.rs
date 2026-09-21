use crate::cli::args::{Cli, Shell};
use crate::error::Result;
use crate::{user_info, user_success};
use clap::CommandFactory;
use clap_complete::{generate, shells};
use std::io::Write;

pub fn handle_completions(shell: Shell, install: bool) -> Result<()> {
    let mut cmd = Cli::command();
    let mut buf = Vec::new();
    match shell {
        Shell::Bash => generate(shells::Bash, &mut cmd, "bt", &mut buf),
        Shell::Zsh => generate(shells::Zsh, &mut cmd, "bt", &mut buf),
        Shell::Fish => generate(shells::Fish, &mut cmd, "bt", &mut buf),
    }

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
        };
        std::fs::create_dir_all(&dir)?;
        std::fs::write(&file, &buf)?;
        if crate::cli::output::is_json() {
            crate::cli::output::print_json(&serde_json::json!({
                "installed": true,
                "shell": format!("{:?}", shell).to_lowercase(),
                "path": file.display().to_string()
            }))?;
        } else {
            user_success!("补全脚本已安装到 {}", file.display());
            match shell {
                Shell::Zsh => user_info!("请在 ~/.zshrc 中添加 fpath=(~/.zsh/completions $fpath)，然后执行 exec zsh 生效"),
                Shell::Bash => user_info!("请执行 source {} 或重新打开终端生效", file.display()),
                Shell::Fish => user_info!("重新打开 fish 终端即可生效"),
            }
        }
    } else {
        std::io::stdout().write_all(&buf)?;
    }
    Ok(())
}
