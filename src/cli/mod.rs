pub mod args;
pub mod auth;
pub mod completions;
pub mod fav;
pub mod feed;
pub mod history;
pub mod later;
pub mod live;
pub mod output;
pub mod rank;
pub mod resolve;
pub mod search;
pub mod user;
pub mod video;

pub use args::{Cli, Commands};
#[allow(unused_imports)]
pub use output::{
    init_output, is_json, is_no_color, is_quiet, is_yes, print_json, print_json_error, set_no_color,
};

use crate::error::Result;

pub(crate) fn metadata_json_value() -> serde_json::Value {
    serde_json::json!({
        "name": "bit",
        "package": env!("CARGO_PKG_NAME"),
        "version": env!("CARGO_PKG_VERSION"),
        "modules": [
            "resolve",
            "auth",
            "live",
            "search",
            "video",
            "user",
            "feed",
            "rank",
            "fav",
            "later",
            "history",
            "completions"
        ],
        "global_flags": [
            "--json",
            "-q/--quiet",
            "-v/--verbose",
            "-y/--yes",
            "--no-color",
            "-u/--cookie-file",
            "-D/--direct"
        ]
    })
}

pub fn print_metadata_json() -> Result<()> {
    print_json(&metadata_json_value())
}

pub fn run(cli: Cli) -> Result<()> {
    crate::utils::paths::set_cookie_file_override(cli.cookie_file.clone());

    let Some(command) = cli.command else {
        if is_json() {
            print_metadata_json()?;
        } else if !is_quiet() {
            crate::ui::banner::print_welcome_help();
        }
        return Ok(());
    };

    match command {
        Commands::Resolve { input, no_network } => resolve::handle_resolve(&input, no_network),
        Commands::Auth(cmd) => auth::handle_auth(cmd),
        Commands::Live(cmd) => live::handle_live(cmd),
        Commands::Search(cmd) => search::handle_search(cmd),
        Commands::Video(cmd) => video::handle_video(cmd),
        Commands::User(cmd) => user::handle_user(cmd),
        Commands::Feed { feed_type, offset } => feed::handle_feed(feed_type, offset),
        Commands::Rank(cmd) => rank::handle_rank(cmd),
        Commands::Fav(cmd) => fav::handle_fav(cmd),
        Commands::Later(cmd) => later::handle_later(cmd),
        Commands::History(cmd) => history::handle_history(cmd),
        Commands::Completions {
            shell,
            install,
            zsh,
            bash,
            fish,
            powershell,
            elvish,
        } => {
            let resolved_shell = shell.or(if zsh {
                Some(args::Shell::Zsh)
            } else if bash {
                Some(args::Shell::Bash)
            } else if fish {
                Some(args::Shell::Fish)
            } else if powershell {
                Some(args::Shell::PowerShell)
            } else if elvish {
                Some(args::Shell::Elvish)
            } else {
                None
            });
            completions::handle_completions(resolved_shell, install)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_json_contains_all_twelve_modules_and_global_flags() {
        let meta = metadata_json_value();
        assert_eq!(meta["name"], "bit");
        let mods = meta["modules"].as_array().unwrap();
        let expected = [
            "resolve",
            "auth",
            "live",
            "search",
            "video",
            "user",
            "feed",
            "rank",
            "fav",
            "later",
            "history",
            "completions",
        ];
        assert_eq!(mods.len(), expected.len());
        for m in expected {
            assert!(
                mods.iter().any(|v| v.as_str() == Some(m)),
                "missing module {m}"
            );
        }
        let flags = meta["global_flags"].as_array().unwrap();
        assert_eq!(flags.len(), 7);
    }
}
