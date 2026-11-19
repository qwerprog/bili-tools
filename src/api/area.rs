use crate::api::client::DEFAULT_USER_AGENT;
use crate::error::Result;

pub fn fetch_area_list() -> Result<serde_json::Value> {
    let url = format!(
        "{}/room/v1/Area/getList",
        crate::api::client::live_api_base_url()
    );
    let response = crate::api::client::get(&url)
        .with_header("User-Agent", DEFAULT_USER_AGENT)
        .send()?;

    let area_list: serde_json::Value =
        crate::api::client::parse_json_response(&response, "获取分区列表失败")?;
    Ok(area_list)
}
