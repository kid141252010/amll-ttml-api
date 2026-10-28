struct FormattedLine {
    start_time_ms: u32,
    text: String,
    is_bg: bool,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct TTMLFormatResult {
    pub plain_lyrics: Option<String>,
    pub synced_lyrics: Option<String>,
    pub duration: f64,
}

pub fn parse_and_format_ttml(raw_ttml: &str) -> TTMLFormatResult {
    let Ok(parsed) = ttml_processor::parse_ttml(raw_ttml) else {
        return TTMLFormatResult::default();
    };

    let mut lines = Vec::new();
    let mut max_end_ms: u32 = 0;

    for line in &parsed.lines {
        if line.end_time > max_end_ms {
            max_end_ms = line.end_time;
        }

        if !line.text.is_empty() {
            lines.push(FormattedLine {
                start_time_ms: line.start_time,
                text: line.text.clone(),
                is_bg: false,
            });
        }

        if let Some(bg) = &line.background_vocal {
            if bg.end_time > max_end_ms {
                max_end_ms = bg.end_time;
            }
            if !bg.text.is_empty() {
                lines.push(FormattedLine {
                    start_time_ms: bg.start_time,
                    text: format!("({})", bg.text),
                    is_bg: true,
                });
            }
        }
    }

    let duration = f64::from(max_end_ms) / 1000.0;

    if lines.is_empty() {
        return TTMLFormatResult {
            plain_lyrics: None,
            synced_lyrics: None,
            duration,
        };
    }

    lines.sort_by(|a, b| {
        a.start_time_ms
            .cmp(&b.start_time_ms)
            .then_with(|| a.is_bg.cmp(&b.is_bg))
    });

    let plain_lyrics = lines
        .iter()
        .map(|l| l.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");

    let synced_lyrics = lines
        .iter()
        .map(|l| {
            let ts = format_lrc_timestamp(l.start_time_ms);
            format!("{ts} {}", l.text)
        })
        .collect::<Vec<_>>()
        .join("\n");

    TTMLFormatResult {
        plain_lyrics: Some(plain_lyrics),
        synced_lyrics: Some(synced_lyrics),
        duration,
    }
}

fn format_lrc_timestamp(ms: u32) -> String {
    let total_seconds = ms / 1000;
    let minutes = total_seconds / 60;
    let seconds = total_seconds % 60;
    let centiseconds = (ms % 1000) / 10;
    format!("[{minutes:02}:{seconds:02}.{centiseconds:02}]")
}

#[cfg(test)]
#[path = "ttml_tests.rs"]
mod tests;
