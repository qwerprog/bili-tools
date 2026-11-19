use crate::cli::args::FeedType;
use crate::cli::output::{is_json, print_json};
use crate::error::Result;

pub fn handle_feed(feed_type: FeedType, offset: Option<String>) -> Result<()> {
    let cookies = crate::auth::ensure_login()?;
    let data =
        crate::api::user::get_feed(&cookies, feed_type.as_api_str(), offset.as_deref())?;

    if is_json() {
        print_json(&data)?;
        return Ok(());
    }

    let type_label = match feed_type {
        FeedType::All => "全部",
        FeedType::Video => "视频",
        FeedType::Pgc => "番剧",
        FeedType::Article => "专栏",
    };
    crate::cli::user::render_dynamics_table(&format!("关注动态 · {type_label}"), &data);
    Ok(())
}
