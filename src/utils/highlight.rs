pub fn extract_lyric_context(lyric_text: &str, keyword: &str) -> Option<String> {
    if lyric_text.trim().is_empty() || keyword.trim().is_empty() {
        return None;
    }

    let lines: Vec<&str> = lyric_text.lines().collect();
    if lines.is_empty() {
        return None;
    }

    let kw_lower = keyword.to_lowercase();

    let matched_idx = lines
        .iter()
        .position(|line| line.to_lowercase().contains(&kw_lower))?;

    let start_idx = matched_idx.saturating_sub(1);
    let end_idx = (matched_idx + 2).min(lines.len());

    let context_lines = &lines[start_idx..end_idx];
    let joined_context = context_lines.join("\n");

    Some(highlight_keyword(&joined_context, keyword))
}

pub fn highlight_keyword(text: &str, keyword: &str) -> String {
    if keyword.is_empty() || text.is_empty() {
        return text.to_string();
    }

    let kw_lower = keyword.to_lowercase();
    let kw_char_count = kw_lower.chars().count();
    let mut result = String::with_capacity(text.len() + 32);
    let mut last_end_byte = 0;

    let char_boundaries: Vec<(usize, char)> = text.char_indices().collect();
    let total_chars = char_boundaries.len();
    let mut i = 0;

    while i < total_chars {
        if i + kw_char_count <= total_chars {
            let start_byte = char_boundaries[i].0;
            let end_byte = if i + kw_char_count < total_chars {
                char_boundaries[i + kw_char_count].0
            } else {
                text.len()
            };

            let candidate = &text[start_byte..end_byte];

            if candidate.to_lowercase() == kw_lower {
                result.push_str(&text[last_end_byte..start_byte]);
                result.push_str("<mark>");
                result.push_str(candidate);
                result.push_str("</mark>");

                last_end_byte = end_byte;
                i += kw_char_count;
                continue;
            }
        }
        i += 1;
    }

    result.push_str(&text[last_end_byte..]);
    result
}

#[cfg(test)]
mod tests;
