// SPDX-License-Identifier: GPL-3.0-or-later

use std::fs;
use crate::audio::Song;
use gtk::glib;
use gtk::prelude::*;

#[derive(Debug, Clone, PartialEq)]
pub struct LyricWord {
    pub time: u64, // ms start
    pub text: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LyricSubLine {
    pub time: u64, // ms start
    pub text: String,
    pub words: Vec<LyricWord>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LyricLine {
    pub time: u64, // ms start
    pub text: String,
    pub words: Vec<LyricWord>,
    pub is_background: bool,
    pub sub_line: Option<LyricSubLine>,
}

#[derive(Debug, Clone, Default)]
pub struct Lyrics {
    pub lines: Vec<LyricLine>,
}

impl LyricLine {
    pub fn generate_karaoke_markup(words: &[LyricWord], current_time_ms: u64) -> String {
        if words.is_empty() {
            return String::new();
        }

        // Find index of the word currently playing
        let active_word_idx = words.iter().rposition(|w| w.time <= current_time_ms);

        let mut past_text = String::new();
        let mut active_text = String::new();
        let mut future_text = String::new();

        // Check if the last word has completed its active window (~800ms)
        let is_current_word_active = if let Some(cur) = active_word_idx {
            if cur + 1 < words.len() {
                true
            } else {
                current_time_ms <= words[cur].time + 800
            }
        } else {
            false
        };

        for (i, w) in words.iter().enumerate() {
            let escaped = glib::markup_escape_text(&w.text);
            match active_word_idx {
                Some(cur) if i < cur => {
                    past_text.push_str(&escaped);
                }
                Some(cur) if i == cur => {
                    if is_current_word_active {
                        active_text.push_str(&escaped);
                    } else {
                        past_text.push_str(&escaped);
                    }
                }
                _ => {
                    future_text.push_str(&escaped);
                }
            }
        }

        let mut result = String::with_capacity(past_text.len() + active_text.len() + future_text.len() + 120);
        if !past_text.is_empty() {
            result.push_str("<span foreground=\"#ffffff\" weight=\"bold\">");
            result.push_str(&past_text);
            result.push_str("</span>");
        }
        if !active_text.is_empty() {
            result.push_str("<span foreground=\"#00ffff\" weight=\"heavy\">");
            result.push_str(&active_text);
            result.push_str("</span>");
        }
        if !future_text.is_empty() {
            result.push_str("<span foreground=\"#7a7a88\" weight=\"medium\">");
            result.push_str(&future_text);
            result.push_str("</span>");
        }

        result
    }
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

    pub fn parse_lrc(content: &str) -> Self {
        let mut lines = Vec::new();
        for raw_line in content.lines() {
            let line = raw_line.trim();
            if line.is_empty() {
                continue;
            }

            // Extract all leading [mm:ss.xx] timestamps (e.g. [00:10.00][00:20.00]text)
            let mut timestamps = Vec::new();
            let mut rest = line;
            while rest.starts_with('[') {
                if let Some(end_bracket) = rest.find(']') {
                    let tag = &rest[1..end_bracket];
                    if let Some(time_ms) = Self::parse_timestamp(tag) {
                        timestamps.push(time_ms);
                        rest = &rest[end_bracket + 1..];
                    } else {
                        // Might be metadata tag like [ar:Artist], skip
                        break;
                    }
                } else {
                    break;
                }
            }

            if timestamps.is_empty() {
                continue;
            }

            let line_time = timestamps[0];
            let mut s = rest.trim();

            // Detect and strip voice prefixes like "v1:", "V1:", "v2:", "[v1]:"
            let mut is_bg_voice = false;
            if let Some(colon_pos) = s.find(':') {
                let prefix = s[..colon_pos].trim_start_matches('[').trim_end_matches(']');
                if (prefix.starts_with('v') || prefix.starts_with('V'))
                    && prefix.len() > 1
                    && prefix[1..].chars().all(|c| c.is_ascii_digit())
                {
                    if let Ok(v_num) = prefix[1..].parse::<u32>() {
                        if v_num > 1 {
                            is_bg_voice = true;
                        }
                    }
                    s = s[colon_pos + 1..].trim();
                }
            }

            // Parse syllable tokens if `<...>` tags exist
            let has_syllables = s.contains('<') && s.contains('>');
            if has_syllables {
                let tokens = Self::extract_syllable_tokens(s, line_time);
                if !tokens.is_empty() {
                    let first_paren = tokens.iter().position(|t| t.text.contains('('));
                    let (main_words, sub_words, is_background) = match first_paren {
                        Some(0) => (tokens, Vec::new(), true),
                        Some(k) => (tokens[..k].to_vec(), tokens[k..].to_vec(), is_bg_voice),
                        None => (tokens, Vec::new(), is_bg_voice),
                    };

                    let main_text = Self::tokens_to_clean_string(&main_words);
                    let sub_line = if !sub_words.is_empty() {
                        let sub_time = sub_words[0].time;
                        let sub_text = Self::tokens_to_clean_string(&sub_words);
                        Some(LyricSubLine {
                            time: sub_time,
                            text: sub_text,
                            words: sub_words,
                        })
                    } else {
                        None
                    };

                    if !main_text.is_empty() {
                        for time in timestamps {
                            lines.push(LyricLine {
                                time,
                                text: main_text.clone(),
                                words: main_words.clone(),
                                is_background,
                                sub_line: sub_line.clone(),
                            });
                        }
                    }
                    continue;
                }
            }

            // Fallback: standard line-timed LRC (no syllable tags)
            let (main_text, sub_text, is_background) = Self::split_plain_background(s, is_bg_voice);
            let sub_line = sub_text.map(|txt| LyricSubLine {
                time: line_time,
                text: txt,
                words: Vec::new(),
            });

            if !main_text.is_empty() {
                for time in timestamps {
                    lines.push(LyricLine {
                        time,
                        text: main_text.clone(),
                        words: Vec::new(),
                        is_background,
                        sub_line: sub_line.clone(),
                    });
                }
            }
        }

        lines.sort_by_key(|l| l.time);

        // If no timed lines found, fallback to plain text lines spaced by 4 seconds
        if lines.is_empty() && !content.trim().is_empty() {
            for (i, raw_line) in content.lines().enumerate() {
                let trimmed = raw_line.trim();
                if !trimmed.is_empty() && !trimmed.starts_with('[') {
                    let clean = Self::clean_plain_text(trimmed);
                    if !clean.is_empty() {
                        lines.push(LyricLine {
                            time: i as u64 * 4000,
                            text: clean,
                            words: Vec::new(),
                            is_background: false,
                            sub_line: None,
                        });
                    }
                }
            }
        }

        Self { lines }
    }

    fn extract_syllable_tokens(s: &str, default_time: u64) -> Vec<LyricWord> {
        let mut tokens = Vec::new();
        let mut cur_time = default_time;
        let mut text_buf = String::new();

        let mut i = 0;
        let bytes = s.as_bytes();
        while i < bytes.len() {
            if bytes[i] == b'<' {
                if let Some(close_rel) = s[i..].find('>') {
                    let tag_content = &s[i + 1..i + close_rel];
                    if let Some(t) = Self::parse_timestamp(tag_content) {
                        if !text_buf.is_empty() {
                            tokens.push(LyricWord {
                                time: cur_time,
                                text: text_buf.clone(),
                            });
                            text_buf.clear();
                        }
                        cur_time = t;
                        i += close_rel + 1;
                        continue;
                    }
                }
            }
            let ch = s[i..].chars().next().unwrap();
            text_buf.push(ch);
            i += ch.len_utf8();
        }
        if !text_buf.is_empty() {
            tokens.push(LyricWord {
                time: cur_time,
                text: text_buf,
            });
        }
        tokens
    }

    fn tokens_to_clean_string(tokens: &[LyricWord]) -> String {
        let mut s = String::new();
        for t in tokens {
            s.push_str(&t.text);
        }
        s.split_whitespace().collect::<Vec<_>>().join(" ")
    }

    fn split_plain_background(s: &str, is_bg_voice: bool) -> (String, Option<String>, bool) {
        let clean = Self::clean_plain_text(s);
        if clean.starts_with('(') && clean.ends_with(')') {
            (clean, None, true)
        } else if let Some(paren_pos) = clean.find('(') {
            let main = clean[..paren_pos].trim().to_string();
            let sub = clean[paren_pos..].trim().to_string();
            (main, Some(sub), is_bg_voice)
        } else {
            (clean, None, is_bg_voice)
        }
    }

    fn clean_plain_text(raw: &str) -> String {
        let mut result = String::with_capacity(raw.len());
        let mut in_tag = false;
        for c in raw.chars() {
            if c == '<' {
                in_tag = true;
            } else if c == '>' {
                in_tag = false;
            } else if !in_tag {
                result.push(c);
            }
        }
        result.split_whitespace().collect::<Vec<_>>().join(" ")
    }

    fn parse_timestamp(tag: &str) -> Option<u64> {
        let time_parts: Vec<&str> = tag.split(':').collect();
        if time_parts.len() < 2 {
            return None;
        }

        let mins: u64 = time_parts[0].trim().parse().ok()?;
        let secs_parts: Vec<&str> = time_parts[1].split(|c| c == '.' || c == ':').collect();
        let secs: u64 = secs_parts[0].trim().parse().ok()?;
        let ms: u64 = if secs_parts.len() > 1 {
            let mut ms_str = secs_parts[1].trim().to_string();
            if ms_str.len() > 3 {
                ms_str.truncate(3);
            }
            while ms_str.len() < 3 {
                ms_str.push('0');
            }
            ms_str.parse().ok()?
        } else {
            0
        };

        Some(mins * 60000 + secs * 1000 + ms)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_lrc_syllable_with_background() {
        let sample = "[00:09.375]v1:<00:09.375>Don't <00:09.687><00:09.687>trip <00:09.855><00:09.855>and <00:10.006><00:10.006>fall <00:10.268><00:10.268>in <00:10.403><00:10.403>your <00:10.575><00:10.575>LB <00:11.161><00:11.161>heels <00:11.403><00:11.403>(got, <00:11.52><00:11.52>got)<00:11.675>";
        let parsed = Lyrics::parse_lrc(sample);
        assert_eq!(parsed.lines.len(), 1);
        let line = &parsed.lines[0];
        assert_eq!(line.time, 9375);
        assert_eq!(line.text, "Don't trip and fall in your LB heels");
        assert!(!line.words.is_empty());
        assert!(line.sub_line.is_some());
        let sub = line.sub_line.as_ref().unwrap();
        assert_eq!(sub.time, 11403);
        assert_eq!(sub.text, "(got, got)");
        assert_eq!(sub.words.len(), 2);
    }

    #[test]
    fn test_parse_lrc_syllables_words_joined() {
        let sample = "[00:11.715]v1:<00:11.715>Bul<00:11.765><00:11.765>lets <00:12.013><00:12.013>make <00:12.158><00:12.158>it <00:12.321><00:12.321>rain <00:12.575><00:12.575>like <00:12.736><00:12.736>a <00:12.894><00:12.894>win<00:12.935><00:12.935>dowsill<00:13.636>";
        let parsed = Lyrics::parse_lrc(sample);
        assert_eq!(parsed.lines.len(), 1);
        let line = &parsed.lines[0];
        assert_eq!(line.time, 11715);
        assert_eq!(line.text, "Bullets make it rain like a windowsill");
        assert!(line.sub_line.is_none());
        assert_eq!(line.words.len(), 9);
    }

    #[test]
    fn test_karaoke_markup() {
        let words = vec![
            LyricWord { time: 1000, text: "Don't ".to_string() },
            LyricWord { time: 2000, text: "worry".to_string() },
        ];
        // Before 1000: all unsung
        let m0 = LyricLine::generate_karaoke_markup(&words, 500);
        assert!(m0.contains("foreground=\"#7a7a88\""));
        assert!(!m0.contains("foreground=\"#00ffff\""));
        assert!(!m0.contains("foreground=\"#ffffff\""));

        // At 1500: "Don't " is active playing (cyan), "worry" unsung (grey)
        let m1 = LyricLine::generate_karaoke_markup(&words, 1500);
        assert!(m1.contains("<span foreground=\"#00ffff\" weight=\"heavy\">Don&apos;t </span>"));
        assert!(m1.contains("<span foreground=\"#7a7a88\" weight=\"medium\">worry</span>"));

        // At 2200: "Don't " is past (white), "worry" is active playing (cyan)
        let m2 = LyricLine::generate_karaoke_markup(&words, 2200);
        assert!(m2.contains("<span foreground=\"#ffffff\" weight=\"bold\">Don&apos;t </span>"));
        assert!(m2.contains("<span foreground=\"#00ffff\" weight=\"heavy\">worry</span>"));

        // At 3500 (after 2000+800ms): all past (white)
        let m3 = LyricLine::generate_karaoke_markup(&words, 3500);
        assert!(m3.contains("<span foreground=\"#ffffff\" weight=\"bold\">Don&apos;t worry</span>"));
        assert!(!m3.contains("foreground=\"#00ffff\""));
    }
}
