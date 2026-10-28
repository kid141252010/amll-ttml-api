use serde::Serialize;

use crate::{
    core::{
        LyricId,
        models::SongEntry,
    },
    utils::ttml::TTMLFormatResult,
};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LrclibSongItem {
    pub id: LyricId,
    pub name: String,
    pub track_name: String,
    pub artist_name: String,
    pub album_name: String,
    pub duration: f64,
    pub instrumental: bool,
    pub plain_lyrics: Option<String>,
    pub synced_lyrics: Option<String>,
}

pub fn map_to_lrclib_item(
    entry: &SongEntry,
    formatted: Option<&TTMLFormatResult>,
) -> LrclibSongItem {
    let track_name = entry
        .track_names
        .first()
        .map(ToString::to_string)
        .unwrap_or_default();

    let (plain_lyrics, synced_lyrics, duration) = formatted.map_or((None, None, 0.0), |f| {
        (f.plain_lyrics.clone(), f.synced_lyrics.clone(), f.duration)
    });

    LrclibSongItem {
        id: entry.id,
        name: track_name.clone(),
        track_name,
        artist_name: entry
            .artist_names
            .first()
            .map(ToString::to_string)
            .unwrap_or_default(),
        album_name: entry
            .album_names
            .first()
            .map(ToString::to_string)
            .unwrap_or_default(),
        duration,
        instrumental: false,
        plain_lyrics,
        synced_lyrics,
    }
}

#[cfg(test)]
#[path = "dto_tests.rs"]
mod tests;
