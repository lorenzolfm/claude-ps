//! What the kernel knows about a process, and the two parsers that read it.
//!
//! Both parsers take bytes and touch no files, because their difficult cases are difficult to
//! make on a real system: a `comm` that contains spaces and parentheses, and an environment
//! entry that is not valid UTF-8.

use std::fs;
use std::path::PathBuf;

fn proc_path(pid: u32, leaf: &str) -> PathBuf {
    let mut path = PathBuf::from("/proc");
    path.push(pid.to_string());
    path.push(leaf);
    path
}

/// The start time of the process, field 22 of `/proc/<pid>/stat`, as the kernel writes it.
///
/// A string and not a number: this tool only compares the value against the value that Claude
/// Code recorded, and a conversion can change it.
pub fn start_time(pid: u32) -> Option<String> {
    let stat = fs::read_to_string(proc_path(pid, "stat")).ok()?;
    parse_start_time(&stat).map(str::to_owned)
}

/// Field 22 of a `/proc/<pid>/stat` line.
///
/// Parse after the last `)`, and do not split the full line. Field 2 is `comm`, the name of the
/// executable. The kernel puts `comm` in parentheses but does not escape it, so a process named
/// `(sd-pam)` shows as `((sd-pam))`, and a name with a space changes the number of fields. The
/// text after the last `)` starts at field 3, so the start time is at index 19.
pub fn parse_start_time(stat: &str) -> Option<&str> {
    let close = stat.rfind(')')?;
    stat[close + 1..].split_whitespace().nth(19)
}

/// The `(ZELLIJ_SESSION_NAME, ZELLIJ_PANE_ID)` of the agent, or `None` for each one that the
/// agent does not have. Only the environment of the agent has these values.
pub fn zellij_of(pid: u32) -> (Option<String>, Option<String>) {
    match fs::read(proc_path(pid, "environ")) {
        Ok(raw) => parse_environ(&raw),
        Err(_) => (None, None),
    }
}

/// `/proc/<pid>/environ` holds `KEY=VALUE` entries that a NUL byte separates. A value is
/// arbitrary bytes.
///
/// A value that is not valid UTF-8 is decoded lossily and not rejected, so a bad session name
/// shows with replacement characters and does not remove the agent from the list.
pub fn parse_environ(raw: &[u8]) -> (Option<String>, Option<String>) {
    let mut session = None;
    let mut pane = None;
    for entry in raw.split(|byte| *byte == 0) {
        let Some(eq) = entry.iter().position(|byte| *byte == b'=') else {
            continue;
        };
        let (key, value) = entry.split_at(eq);
        let value = || String::from_utf8_lossy(&value[1..]).into_owned();
        match key {
            b"ZELLIJ_SESSION_NAME" => session = Some(value()),
            b"ZELLIJ_PANE_ID" => pane = Some(value()),
            _ => {}
        }
    }
    (session, pane)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Fields 3 to 21 of a real line, so the number of fields between the `)` and the start
    /// time comes from the kernel.
    const FIELDS_3_TO_21: &str = "R 1 1 1 0 -1 4194304 328 0 0 0 0 0 0 0 20 0 1 0";

    #[test]
    fn start_time_is_field_22() {
        let stat = format!("3520542 (cat) {FIELDS_3_TO_21} 41288167 17489920 1165");
        assert_eq!(parse_start_time(&stat), Some("41288167"));
    }

    #[test]
    fn start_time_survives_parens_and_spaces_in_comm() {
        let stat = format!("1 ((sd pam)) {FIELDS_3_TO_21} 555 17489920 1165");
        assert_eq!(parse_start_time(&stat), Some("555"));
    }

    #[test]
    fn start_time_rejects_a_truncated_line() {
        assert_eq!(parse_start_time("42 (claude) S 0 0"), None);
        assert_eq!(parse_start_time("no parens here"), None);
    }

    #[test]
    fn environ_finds_both_zellij_vars() {
        let raw = b"PATH=/usr/bin\0ZELLIJ_SESSION_NAME=work\0ZELLIJ_PANE_ID=3\0";
        let (session, pane) = parse_environ(raw);
        assert_eq!(session.as_deref(), Some("work"));
        assert_eq!(pane.as_deref(), Some("3"));
    }

    #[test]
    fn environ_without_zellij_yields_nothing() {
        assert_eq!(parse_environ(b"PATH=/usr/bin\0HOME=/root\0"), (None, None));
    }

    #[test]
    fn environ_ignores_malformed_entries() {
        let raw = b"JUSTAKEY\0\0ZELLIJ_PANE_ID=0\0";
        let (session, pane) = parse_environ(raw);
        assert_eq!(session, None);
        assert_eq!(pane.as_deref(), Some("0"));
    }

    #[test]
    fn environ_decodes_invalid_utf8_lossily() {
        let raw = b"ZELLIJ_SESSION_NAME=bad\xffname\0";
        let (session, _) = parse_environ(raw);
        assert_eq!(session.as_deref(), Some("bad\u{fffd}name"));
    }

    #[test]
    fn environ_splits_on_the_first_equals_only() {
        let (session, _) = parse_environ(b"ZELLIJ_SESSION_NAME=a=b=c\0");
        assert_eq!(session.as_deref(), Some("a=b=c"));
    }
}
