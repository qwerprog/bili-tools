pub mod args;
pub mod auth;
pub mod completions;
pub mod fav;
pub mod history;
pub mod later;
pub mod live;
pub mod output;
pub mod search;
pub mod video;

pub use args::{Cli, Commands};
#[allow(unused_imports)]
pub use output::{init_output, is_json, is_quiet, print_json};

use crate::error::Result;

pub fn run(cli: Cli) -> Result<()> {
    match cli.command {
        Commands::Auth(cmd) => auth::handle_auth(cmd),
        Commands::Live(cmd) => live::handle_live(cmd),
        Commands::Search(cmd) => search::handle_search(cmd),
        Commands::Video(cmd) => video::handle_video(cmd),
        Commands::Fav(cmd) => fav::handle_fav(cmd),
        Commands::Later(cmd) => later::handle_later(cmd),
        Commands::History(cmd) => history::handle_history(cmd),
        Commands::Completions { shell, install } => {
            completions::handle_completions(shell, install)
        }
    }
}
