//! The token count Claude Code does not put in the session registry.
//!
//! 🔴 **This is the tool's only inexact join, and it is deliberately kept separate from the
//! others.** The registry join is provable — a pid and its start time. This one derives a path
//! from `cwd`, and Claude Code builds that path by replacing both `/` **and** `.` with `-`,
//! which is not injective: `/home/x/.config` and `/home/x-config` land on the same directory.
//! So a hit is "almost certainly this session's transcript", never "provably". It costs one
//! key when it is wrong, and the key is `null` when the file is not there at all.
//!
//! ⚠️ What is **not** here is a percentage. The context window size lives only in the payload
//! Claude Code hands a status line at render time; it is computed in-process and never written
//! down. A model-name lookup table could manufacture a denominator, and would then be
//! confidently wrong the day a model ships that the table predates — the same failure the
//! status vocabulary is passed through untouched to avoid. Consumers get the numerator.

use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// How much of the tail to read looking for the last assistant turn.
///
/// ⚠️ Bounded on purpose: a transcript grows without limit — a working session reaches a
/// megabyte in an afternoon — and `claude-tray` polls this every few seconds across every
/// agent at once. Reading whole files would make the cost of running this tool scale with how
/// long you have been working. The last assistant turn is at the end by construction, so a
/// window this size finds it for any conversation that is not one enormous message.
const TAIL_BYTES: u64 = 256 * 1024;

/// How loaded a session's context is, in tokens.
#[derive(Serialize, Debug, Clone, Copy, PartialEq, Eq)]
pub struct Context {
    /// Everything sent for the most recent assistant turn: fresh input, cache writes and cache
    /// reads. 🔴 Output tokens are **not** added — they are what the *next* request will carry,
    /// and counting them here would overstate what is in the window now.
    pub tokens: u64,
    /// Epoch seconds of that turn.
    ///
    /// ⚠️ This value lags by design and the stamp is how a consumer can tell. It is measured at
    /// the last *completed* assistant turn, so a session that is `busy` right now has been
    /// growing its context since — which is exactly when someone is watching.
    pub as_of: u64,
}

/// `~/.claude/projects/<slug>/<session_id>.jsonl`, where the slug is `cwd` with every `/` and
/// every `.` replaced by `-`.
fn transcript_path(home: &str, cwd: &str, session_id: &str) -> PathBuf {
    let slug: String = cwd
        .chars()
        .map(|c| if c == '/' || c == '.' { '-' } else { c })
        .collect();
    PathBuf::from(home)
        .join(".claude")
        .join("projects")
        .join(slug)
        .join(format!("{session_id}.jsonl"))
}

/// The context this session was carrying at its last assistant turn.
pub fn context_of(home: &str, cwd: &str, session_id: &str) -> Option<Context> {
    let text = tail(&transcript_path(home, cwd, session_id), TAIL_BYTES)?;
    last_assistant_usage(&text)
}

/// The final `max` bytes of a file, starting at a line boundary.
///
/// A read that began mid-file drops its first line, which is the half of a record the offset
/// landed inside. Decoded lossily for the same reason the environment is: a mangled byte
/// somewhere in a transcript should not cost the whole answer.
fn tail(path: &PathBuf, max: u64) -> Option<String> {
    let mut file = fs::File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    let start = len.saturating_sub(max);
    file.seek(SeekFrom::Start(start)).ok()?;

    let mut buf = Vec::new();
    file.read_to_end(&mut buf).ok()?;
    let text = String::from_utf8_lossy(&buf).into_owned();

    if start == 0 {
        return Some(text);
    }
    let first_break = text.find('\n')?;
    Some(text[first_break + 1..].to_string())
}

/// One transcript record, as much of it as this module reads.
#[derive(Deserialize)]
struct Entry {
    #[serde(rename = "type", default)]
    kind: Option<String>,
    /// 🔴 Subagent turns are written into the **same** file, and their usage is the subagent's
    /// own context rather than this session's. Counting one would report a number that belongs
    /// to a conversation the user cannot see.
    #[serde(rename = "isSidechain", default)]
    is_sidechain: bool,
    #[serde(default)]
    timestamp: Option<String>,
    #[serde(default)]
    message: Option<Message>,
}

#[derive(Deserialize)]
struct Message {
    #[serde(default)]
    usage: Option<Usage>,
}

#[derive(Deserialize)]
struct Usage {
    #[serde(default)]
    input_tokens: u64,
    #[serde(default)]
    cache_creation_input_tokens: u64,
    #[serde(default)]
    cache_read_input_tokens: u64,
}

/// Scan **backwards** for the newest assistant turn that reported usage.
///
/// Backwards because the answer is the last one, and a transcript has thousands of records
/// before it. Records that will not parse are skipped rather than ending the scan: this file is
/// appended to by another process while this one reads it, so the final line is regularly half
/// written.
pub fn last_assistant_usage(text: &str) -> Option<Context> {
    for line in text.lines().rev() {
        let Ok(entry) = serde_json::from_str::<Entry>(line) else {
            continue;
        };
        if entry.kind.as_deref() != Some("assistant") || entry.is_sidechain {
            continue;
        }
        let Some(usage) = entry.message.and_then(|m| m.usage) else {
            continue;
        };
        return Some(Context {
            tokens: usage.input_tokens
                + usage.cache_creation_input_tokens
                + usage.cache_read_input_tokens,
            as_of: entry
                .timestamp
                .as_deref()
                .and_then(iso_epoch_secs)
                .unwrap_or(0),
        });
    }
    None
}

/// `2026-08-30T01:10:21.036Z` as epoch seconds.
///
/// Hand-rolled rather than pulling in a date crate for one field: the format is fixed, it is
/// always UTC, and only the whole seconds are wanted. An unparseable stamp is `0`, on the same
/// reasoning as an undated age — 1970 reads as breakage where "now" would read as data.
fn iso_epoch_secs(iso: &str) -> Option<u64> {
    let bytes = iso.as_bytes();
    if bytes.len() < 19 || bytes[4] != b'-' || bytes[7] != b'-' || bytes[10] != b'T' {
        return None;
    }
    let num = |a: usize, b: usize| iso.get(a..b)?.parse::<i64>().ok();
    let (y, m, d) = (num(0, 4)?, num(5, 7)?, num(8, 10)?);
    let (hh, mm, ss) = (num(11, 13)?, num(14, 16)?, num(17, 19)?);

    // Days from the civil date, by Howard Hinnant's algorithm: shift the year to start in
    // March so a leap day is the last day of the year and needs no special case.
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;

    let secs = days * 86_400 + hh * 3_600 + mm * 60 + ss;
    u64::try_from(secs).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 🔴 Both separators collapse to `-`, which is why this derivation is a good guess and not
    /// a proof. `.config` becoming `--config` is the visible shape of that.
    #[test]
    fn the_slug_replaces_both_slashes_and_dots() {
        let path = transcript_path("/home/you", "/home/you/.config/nixos", "abc");
        assert!(path.ends_with("projects/-home-you--config-nixos/abc.jsonl"));

        let path = transcript_path("/home/you", "/home/you/Work/infra.git/master", "abc");
        assert!(path.ends_with("projects/-home-you-Work-infra-git-master/abc.jsonl"));
    }

    const ASSISTANT: &str = r#"{"type":"assistant","isSidechain":false,"timestamp":"2026-08-30T01:10:21.036Z","message":{"usage":{"input_tokens":2,"cache_creation_input_tokens":2015,"cache_read_input_tokens":185936,"output_tokens":905}}}"#;

    #[test]
    fn sums_the_three_input_kinds_and_not_the_output() {
        let context = last_assistant_usage(ASSISTANT).unwrap();
        assert_eq!(context.tokens, 2 + 2015 + 185_936);
        assert_eq!(context.as_of, 1_788_052_221);
    }

    /// The newest turn wins, and the scan runs from the end to find it.
    #[test]
    fn the_last_assistant_turn_is_the_answer() {
        let older = ASSISTANT.replace("185936", "1000");
        let text = format!("{older}\n{ASSISTANT}\n");
        assert_eq!(last_assistant_usage(&text).unwrap().tokens, 187_953);
    }

    /// 🔴 A subagent's context is not this session's, and both are written to the same file.
    #[test]
    fn a_sidechain_turn_is_not_this_sessions_context() {
        let sidechain = ASSISTANT.replace(r#""isSidechain":false"#, r#""isSidechain":true"#);
        assert_eq!(last_assistant_usage(&sidechain), None);

        // ...and it does not shadow the real answer sitting behind it.
        let text = format!("{ASSISTANT}\n{sidechain}\n");
        assert_eq!(last_assistant_usage(&text).unwrap().tokens, 187_953);
    }

    /// The file is appended to while this reads it, so a torn final line is ordinary.
    #[test]
    fn a_half_written_line_is_skipped_not_fatal() {
        let text = format!("{ASSISTANT}\n{{\"type\":\"assis");
        assert_eq!(last_assistant_usage(&text).unwrap().tokens, 187_953);
    }

    /// User turns carry no usage, and a transcript with none at all has no answer to give.
    #[test]
    fn no_assistant_turn_is_none_not_zero() {
        assert_eq!(last_assistant_usage(r#"{"type":"user"}"#), None);
        assert_eq!(last_assistant_usage(""), None);
    }

    #[test]
    fn iso_stamps_convert_to_epoch_seconds() {
        assert_eq!(iso_epoch_secs("1970-01-01T00:00:00.000Z"), Some(0));
        assert_eq!(
            iso_epoch_secs("2026-08-30T01:10:21.036Z"),
            Some(1_788_052_221)
        );
        // A leap day, which is what the March-shifted year arithmetic exists for.
        assert_eq!(
            iso_epoch_secs("2024-02-29T12:00:00.000Z"),
            Some(1_709_208_000)
        );
        assert_eq!(iso_epoch_secs("nonsense"), None);
    }
}
