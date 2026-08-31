//! What the kernel knows about a process, and the two parsers that read it.
//!
//! Both parsers take bytes and touch no files, because their difficult cases are difficult to
//! make on a real system: a `comm` that contains spaces and parentheses, and an environment
//! entry that is not valid UTF-8.

fn proc_path(pid: u32, leaf: &str) -> std::path::PathBuf {
    let mut path = std::path::PathBuf::from("/proc");
    path.push(pid.to_string());
    path.push(leaf);
    path
}

/// The start time of the process, field 22 of `/proc/<pid>/stat`, as the kernel writes it.
///
/// A string and not a number: this tool only compares the value against the value that Claude
/// Code recorded, and a conversion can change it.
pub fn start_time(pid: u32) -> Option<String> {
    let stat = std::fs::read_to_string(proc_path(pid, "stat")).ok()?;
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
    match std::fs::read(proc_path(pid, "environ")) {
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

/// The pid domain of this machine, in the spelling Claude Code writes into `pidDomain`.
///
/// `linux:<machine id>:<pid namespace>`, for example
/// `linux:b2ebdff1356e437dae8ff5f78c20e8ff:pid:[4026531836]`.
///
/// Read once. Neither half changes while this process runs, and every session file compares
/// against the same value.
pub fn local_pid_domain() -> Option<&'static str> {
    static DOMAIN: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
    DOMAIN.get_or_init(read_pid_domain).as_deref()
}

fn read_pid_domain() -> Option<String> {
    let machine = machine_id()?;
    let namespace = std::fs::read_link("/proc/self/ns/pid").ok()?;
    Some(format!("linux:{machine}:{}", namespace.to_string_lossy()))
}

/// The machine id, from the systemd path and then from the dbus path that older systems have.
fn machine_id() -> Option<String> {
    ["/etc/machine-id", "/var/lib/dbus/machine-id"]
        .into_iter()
        .find_map(|path| std::fs::read_to_string(path).ok())
        .map(|id| id.trim().to_owned())
        .filter(|id| !id.is_empty())
}

/// The permission mode that the command line of the agent asks for, or `None` for a command
/// line that does not ask for one.
pub fn permission_mode(pid: u32) -> Option<String> {
    let raw = std::fs::read(proc_path(pid, "cmdline")).ok()?;
    parse_permission_mode(&raw)
}

/// `/proc/<pid>/cmdline` holds the arguments as NUL separated entries, like the environment.
///
/// Three spellings set the mode: `--permission-mode <mode>`, `--permission-mode=<mode>`, and
/// `--dangerously-skip-permissions`.
///
/// The mode is passed through verbatim, and this parser compares it against no known set.
/// Claude Code adds modes in new releases, for the same reason that it adds a status.
///
/// `--dangerously-skip-permissions` wins over an explicit mode. A command line that carries
/// both contradicts itself, and the wider of the two is the one a person needs to see.
///
/// Entries are compared whole, and never as a prefix of the argument.
/// `--allow-dangerously-skip-permissions` is a different flag, which permits the bypass and
/// does not perform it.
///
/// A flag with nothing after it gives an empty mode, and this parser reports it. What an empty
/// value means is the decision of the parse boundary in [`crate::agent`], and a parser that
/// hides one emptiness leaves the next one for somebody else to find.
pub fn parse_permission_mode(raw: &[u8]) -> Option<String> {
    const FLAG: &[u8] = b"--permission-mode";

    let mut mode = None;
    let mut args = raw.split(|byte| *byte == 0);
    while let Some(arg) = args.next() {
        if arg == b"--dangerously-skip-permissions" {
            return Some("bypassPermissions".to_owned());
        }
        let value = if arg == FLAG {
            args.next()
        } else {
            arg.strip_prefix(FLAG)
                .and_then(|rest| rest.strip_prefix(b"="))
        };
        if let Some(value) = value {
            mode = Some(String::from_utf8_lossy(value).into_owned());
        }
    }
    mode
}

#[cfg(test)]
mod tests {
    /// Fields 3 to 21 of a real line, so the number of fields between the `)` and the start
    /// time comes from the kernel.
    const FIELDS_3_TO_21: &str = "R 1 1 1 0 -1 4194304 328 0 0 0 0 0 0 0 20 0 1 0";

    #[test]
    fn start_time_is_field_22() {
        let stat = format!("3520542 (cat) {FIELDS_3_TO_21} 41288167 17489920 1165");
        assert_eq!(super::parse_start_time(&stat), Some("41288167"));
    }

    #[test]
    fn start_time_survives_parens_and_spaces_in_comm() {
        let stat = format!("1 ((sd pam)) {FIELDS_3_TO_21} 555 17489920 1165");
        assert_eq!(super::parse_start_time(&stat), Some("555"));
    }

    #[test]
    fn start_time_rejects_a_truncated_line() {
        assert_eq!(super::parse_start_time("42 (claude) S 0 0"), None);
        assert_eq!(super::parse_start_time("no parens here"), None);
    }

    #[test]
    fn environ_finds_both_zellij_vars() {
        let raw = b"PATH=/usr/bin\0ZELLIJ_SESSION_NAME=work\0ZELLIJ_PANE_ID=3\0";
        let (session, pane) = super::parse_environ(raw);
        assert_eq!(session.as_deref(), Some("work"));
        assert_eq!(pane.as_deref(), Some("3"));
    }

    #[test]
    fn environ_without_zellij_yields_nothing() {
        assert_eq!(
            super::parse_environ(b"PATH=/usr/bin\0HOME=/root\0"),
            (None, None)
        );
    }

    #[test]
    fn environ_ignores_malformed_entries() {
        let raw = b"JUSTAKEY\0\0ZELLIJ_PANE_ID=0\0";
        let (session, pane) = super::parse_environ(raw);
        assert_eq!(session, None);
        assert_eq!(pane.as_deref(), Some("0"));
    }

    #[test]
    fn environ_decodes_invalid_utf8_lossily() {
        let raw = b"ZELLIJ_SESSION_NAME=bad\xffname\0";
        let (session, _) = super::parse_environ(raw);
        assert_eq!(session.as_deref(), Some("bad\u{fffd}name"));
    }

    #[test]
    fn a_command_line_without_the_flag_asks_for_no_mode() {
        assert_eq!(super::parse_permission_mode(b"claude\0--resume\0"), None);
        assert_eq!(super::parse_permission_mode(b""), None);
    }

    #[test]
    fn the_mode_is_read_from_both_spellings_of_the_flag() {
        assert_eq!(
            super::parse_permission_mode(b"claude\0--permission-mode\0plan\0").as_deref(),
            Some("plan")
        );
        assert_eq!(
            super::parse_permission_mode(b"claude\0--permission-mode=plan\0").as_deref(),
            Some("plan")
        );
    }

    #[test]
    fn the_bypass_flag_is_a_mode() {
        assert_eq!(
            super::parse_permission_mode(b"claude\0--dangerously-skip-permissions\0").as_deref(),
            Some("bypassPermissions")
        );
    }

    /// `--allow-dangerously-skip-permissions` permits the bypass and does not perform it. An
    /// agent that carries it runs under the mode it asks for, and not under a bypass.
    #[test]
    fn the_flag_that_only_permits_the_bypass_is_not_the_bypass() {
        assert_eq!(
            super::parse_permission_mode(b"claude\0--allow-dangerously-skip-permissions\0"),
            None
        );
        assert_eq!(
            super::parse_permission_mode(
                b"claude\0--allow-dangerously-skip-permissions\0--permission-mode\0plan\0"
            )
            .as_deref(),
            Some("plan")
        );
    }

    #[test]
    fn the_bypass_wins_over_a_narrower_mode_in_either_order() {
        assert_eq!(
            super::parse_permission_mode(
                b"claude\0--permission-mode\0plan\0--dangerously-skip-permissions\0"
            )
            .as_deref(),
            Some("bypassPermissions")
        );
        assert_eq!(
            super::parse_permission_mode(
                b"claude\0--dangerously-skip-permissions\0--permission-mode\0plan\0"
            )
            .as_deref(),
            Some("bypassPermissions")
        );
    }

    /// The mode vocabulary is open, like the status vocabulary.
    #[test]
    fn an_unknown_mode_passes_through() {
        assert_eq!(
            super::parse_permission_mode(b"claude\0--permission-mode\0somethingNew\0").as_deref(),
            Some("somethingNew")
        );
    }

    /// `--permission-mode=` asks for a mode and names none. The parser reports the empty
    /// value, and [`crate::agent::Text::word`] is the one place that decides it is an absence.
    #[test]
    fn a_flag_without_a_value_asks_for_no_mode() {
        assert_eq!(
            super::parse_permission_mode(b"claude\0--permission-mode"),
            None
        );
        assert_eq!(
            super::parse_permission_mode(b"claude\0--permission-mode=\0").as_deref(),
            Some("")
        );
        assert_eq!(crate::agent::Text::word(Some("")), None);
    }

    /// The last one wins, which is what a shell alias that appends a flag produces.
    #[test]
    fn the_last_mode_wins() {
        assert_eq!(
            super::parse_permission_mode(
                b"claude\0--permission-mode\0plan\0--permission-mode\0acceptEdits\0"
            )
            .as_deref(),
            Some("acceptEdits")
        );
    }

    /// The two halves that this tool compares against a session file, so that a wrong shape
    /// shows here and not as an agent that went missing.
    #[test]
    fn the_local_pid_domain_names_this_machine_and_this_namespace() {
        let Some(domain) = super::local_pid_domain() else {
            return;
        };
        assert!(domain.starts_with("linux:"), "{domain}");
        assert!(domain.contains(":pid:["), "{domain}");
    }

    #[test]
    fn environ_splits_on_the_first_equals_only() {
        let (session, _) = super::parse_environ(b"ZELLIJ_SESSION_NAME=a=b=c\0");
        assert_eq!(session.as_deref(), Some("a=b=c"));
    }
}
