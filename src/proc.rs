#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
use linux as os;

#[cfg(not(target_os = "linux"))]
compile_error!("claude-ps needs a process backend for this operating system");

#[derive(serde::Serialize, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[serde(transparent)]
pub struct LivePid(u32);

impl LivePid {
    #[cfg(test)]
    pub fn unchecked(pid: u32) -> Self {
        Self(pid)
    }
}

impl std::fmt::Display for LivePid {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(formatter)
    }
}

pub fn live_pid(pid: u32, recorded_start: &str) -> Option<LivePid> {
    (os::start_time(pid)? == recorded_start).then_some(LivePid(pid))
}

pub fn zellij_of(pid: LivePid) -> (Option<String>, Option<String>) {
    match os::environ(pid.0) {
        Some(raw) => parse_environ(&raw),
        None => (None, None),
    }
}

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

pub fn local_pid_domain() -> Option<&'static str> {
    static DOMAIN: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
    DOMAIN.get_or_init(os::pid_domain).as_deref()
}

pub fn permission_mode(pid: LivePid) -> Option<String> {
    let raw = os::cmdline(pid.0)?;
    parse_permission_mode(&raw)
}

pub fn parse_permission_mode(raw: &[u8]) -> Option<String> {
    const FLAG: &[u8] = b"--permission-mode";

    let mut mode = None;
    let mut args = raw.split(|byte| *byte == 0);
    while let Some(arg) = args.next() {
        if arg == b"--" {
            break;
        }
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
    #[test]
    fn a_checked_pid_is_a_bare_number() {
        let pid = super::LivePid::unchecked(4242);
        assert_eq!(serde_json::to_string(&pid).unwrap(), "4242");
        assert_eq!(pid.to_string(), "4242");
    }

    #[test]
    fn a_pid_is_live_only_when_the_start_time_agrees() {
        let actual = super::os::start_time(std::process::id()).unwrap();
        assert!(super::live_pid(std::process::id(), &actual).is_some());
        assert!(super::live_pid(std::process::id(), "0").is_none());
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

    #[test]
    fn a_prompt_after_a_double_dash_is_not_a_flag() {
        assert_eq!(
            super::parse_permission_mode(b"claude\0-p\0--\0--dangerously-skip-permissions\0"),
            None
        );
        assert_eq!(
            super::parse_permission_mode(b"claude\0-p\0--\0--permission-mode\0plan\0"),
            None
        );
    }

    #[test]
    fn a_mode_before_the_double_dash_still_counts() {
        assert_eq!(
            super::parse_permission_mode(
                b"claude\0--permission-mode\0plan\0--\0--dangerously-skip-permissions\0"
            )
            .as_deref(),
            Some("plan")
        );
    }

    #[test]
    fn an_unknown_mode_passes_through() {
        assert_eq!(
            super::parse_permission_mode(b"claude\0--permission-mode\0somethingNew\0").as_deref(),
            Some("somethingNew")
        );
    }

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

    #[test]
    fn environ_splits_on_the_first_equals_only() {
        let (session, _) = super::parse_environ(b"ZELLIJ_SESSION_NAME=a=b=c\0");
        assert_eq!(session.as_deref(), Some("a=b=c"));
    }
}
