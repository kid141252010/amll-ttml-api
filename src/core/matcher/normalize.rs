use std::sync::OnceLock;

use ferrous_opencc::{
    OpenCC,
    config::BuiltinConfig,
};

static OPENCC: OnceLock<OpenCC> = OnceLock::new();

fn get_opencc() -> &'static OpenCC {
    OPENCC.get_or_init(|| {
        // 必须使用纯繁转简的 T2s，而不能使用 Tw2s：Tw2s 的台湾异体字规范化
        // 会把简体「么」单向改成「幺」。库内 normalized 名称保留「么」，
        // 会导致含「么」的检索词粗筛永远落空。
        OpenCC::from_config(BuiltinConfig::T2s).expect("Failed to initialize OpenCC dictionary")
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
