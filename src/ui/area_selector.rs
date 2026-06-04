use crate::api::area::fetch_area_list;
use crate::error::{BiliLiveError, Result};
use dialoguer::{Select, theme::ColorfulTheme};

// 交互式分区选择器：两级 dialoguer::Select 菜单
pub fn get_area_choice() -> Result<u32> {
    let area_list = fetch_area_list()?;
    let data = area_list["data"]
        .as_array()
        .ok_or_else(|| BiliLiveError::Parse("无法解析分区列表".to_string()))?;

    let areas: Vec<(&str, Vec<(&str, &str)>)> = data
        .iter()
        .map(|a| {
            let name = a["name"].as_str().unwrap_or("");
            let subs: Vec<(&str, &str)> = a["list"]
                .as_array()
                .map(|arr| {
                    arr.iter()
                        .map(|s| {
                            (
                                s["name"].as_str().unwrap_or(""),
                                s["id"].as_str().unwrap_or(""),
                            )
                        })
                        .collect()
                })
                .unwrap_or_default();
            (name, subs)
        })
        .collect();

    // 一级菜单：选择分区大类
    let parent_names: Vec<&str> = areas.iter().map(|(n, _)| *n).collect();
    let parent_idx = Select::with_theme(&ColorfulTheme::default())
        .with_prompt("选择分区")
        .default(0)
        .items(&parent_names)
        .interact()
        .map_err(|e| BiliLiveError::Input(format!("选择分区失败: {e}")))?;

    let (_, subs) = &areas[parent_idx];
    if subs.is_empty() {
        return Err(BiliLiveError::Input(format!(
            "分区 '{}' 没有子分区",
            parent_names[parent_idx]
        )));
    }

    // 二级菜单：选择子分区
    let sub_items: Vec<String> = subs
        .iter()
        .map(|(n, id)| format!("{} - {}", n, id))
        .collect();
    let sub_idx = Select::with_theme(&ColorfulTheme::default())
        .with_prompt(format!("选择子分区 - {}", parent_names[parent_idx]))
        .default(0)
        .items(&sub_items)
        .interact()
        .map_err(|e| BiliLiveError::Input(format!("选择子分区失败: {e}")))?;

    let numeric_id: String = subs[sub_idx].1.chars().filter(|c| c.is_numeric()).collect();
    numeric_id
        .parse::<u32>()
        .map_err(|e| BiliLiveError::Parse(format!("分区ID转换失败: {}", e)))
}
