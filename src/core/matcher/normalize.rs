use std::sync::OnceLock;

use ferrous_opencc::{
    OpenCC,
    config::BuiltinConfig,
};

static OPENCC: OnceLock<OpenCC> = OnceLock::new();

fn get_opencc() -> &'static OpenCC {
    OPENCC.get_or_init(|| {
        OpenCC::from_config(BuiltinConfig::Tw2s).expect("Failed to initialize OpenCC dictionary")
    })
}

pub fn convert_tw2s(input: &str) -> String {
    get_opencc().convert(input)
}

/// 归一化名称字符串（假设输入已为简体中文或已完成繁简转换预处理）
pub fn normalize_name_for_comparison(name: &str) -> String {
    let replaced = name
        .to_lowercase()
        .replace('’', "'")
        .replace('，', ",")
        .replace(['（', '【', '['], " (")
        .replace(['）', '】', ']'], ") ")
        .replace("acoustic version", "acoustic");

    replaced.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
#[path = "normalize_tests.rs"]
mod tests;
