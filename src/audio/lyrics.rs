// SPDX-License-Identifier: GPL-3.0-or-later

use std::fs;
use crate::audio::Song;
use gtk::prelude::*;

#[derive(Debug, Clone)]
pub struct LyricLine {
    pub time: u64, // ms
    pub text: String,
}

#[derive(Debug, Clone, Default)]
pub struct Lyrics {
    pub lines: Vec<LyricLine>,
}

impl Lyrics {
    pub fn from_song(song: &Song) -> Self {
        // First, check for sidecar .lrc file
        if let Some(path) = song.file().path() {
            let mut lrc_path = path.clone();
            lrc_path.set_extension("lrc");
            if lrc_path.exists() {
                if let Ok(content) = fs::read_to_string(&lrc_path) {
                    return Self::parse_lrc(&content);
                }
            }
        }

        // Second, check for embedded lyrics (already extracted in SongData)
        let embedded = song.lyrics();
        if !embedded.is_empty() {
            return Self::parse_lrc(&embedded);
        }

        Self::default()
    }

    fn parse_lrc(content: &str) -> Self {
        let mut lines = Vec::new();
        for line in content.lines() {
            if let Some(lyric) = Self::parse_lrc_line(line) {
                lines.push(lyric);
            }
        }
        lines.sort_by_key(|l| l.time);
        
        // If no timed lines found, maybe it's just plain text
        if lines.is_empty() && !content.trim().is_empty() {
            for (i, line) in content.lines().enumerate() {
                let trimmed = line.trim();
                if !trimmed.is_empty() {
                    lines.push(LyricLine {
                        time: i as u64 * 5000, // Dummy timing: 5s per line for plain text
                        text: trimmed.to_string(),
                    });
                }
            }
        }

        Self { lines }
    }

    fn parse_lrc_line(line: &str) -> Option<LyricLine> {
        // Matches [mm:ss.xx] or [mm:ss:xx] or [mm:ss]
        if !line.starts_with('[') { return None; }
        let end_bracket = line.find(']')?;
        let time_str = &line[1..end_bracket];
        let text = &line[end_bracket+1..].trim();
        
        let time_parts: Vec<&str> = time_str.split(':').collect();
        if time_parts.len() < 2 { return None; }
        
        let mins: u64 = time_parts[0].parse().ok()?;
        let secs_parts: Vec<&str> = time_parts[1].split(|c| c == '.' || c == ':').collect();
        let secs: u64 = secs_parts[0].parse().ok()?;
        let ms: u64 = if secs_parts.len() > 1 {
            let mut ms_str = secs_parts[1].to_string();
            while ms_str.len() < 3 { ms_str.push('0'); }
            ms_str[0..3].parse().ok()?
        } else {
            0
        };

        Some(LyricLine {
            time: mins * 60000 + secs * 1000 + ms,
            text: text.to_string(),
        })
    }
}
