//! The session file Claude Code writes, and the agent this tool prints for it.

/// `~/.claude/sessions/<pid>.json`, as much of it as this tool reads.
///
/// All fields are optional, and unknown fields are ignored. This schema belongs to Claude Code
/// and changes with its releases. A field that goes away costs one key, and not the agent.
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionFile {
    pub pid: Option<u32>,
    /// Compared against `/proc/<pid>/stat` to find if the process is alive. Untyped, because
    /// Claude Code can write this value as a number or as a string.
    pub proc_start: Option<serde_json::Value>,
    /// The machine and the pid namespace that [`SessionFile::pid`] is a name in, for example
    /// `linux:b2ebdff1356e437dae8ff5f78c20e8ff:pid:[4026531836]`. Compared against this
    /// machine before the pid is used at all.
    pub pid_domain: Option<String>,
    /// Passed through verbatim. The vocabulary is open and changes with the version of Claude
    /// Code, so this tool does not compare the status against a known set of values.
    pub status: Option<String>,
    /// Claude's own label for the session, for example `zellij-f8`. This is the basename of the
    /// cwd and a suffix. It is not the zellij session name.
    pub name: Option<String>,
    /// Who chose the name: `user`, `peer`, `derived`, `collision`, `auto`, or `hook`. Passed
    /// through verbatim, and the vocabulary is open, like the status vocabulary.
    ///
    /// It says whether the name carries information. A `derived` name is the basename of the
    /// cwd and a suffix, which a consumer that already shows the cwd shows twice.
    pub name_source: Option<String>,
    pub session_id: Option<String>,
    pub cwd: Option<String>,
    /// Epoch milliseconds. The first of these three fields that is present dates the status.
    pub status_updated_at: Option<i64>,
    pub updated_at: Option<i64>,
    /// Epoch milliseconds when the session started. It dates the status as a last resort, and it
    /// is also an output key: a new session and a session that completed a turn are both `idle`
    /// with a small `status_age`, and only this field makes them different.
    pub started_at: Option<i64>,
}

impl SessionFile {
    /// Whether the pid in this file is still the process that wrote the file.
    ///
    /// All three conditions are necessary. A pid is a name in one pid namespace on one machine,
    /// so the domain has to agree before the pid means anything here. Linux then recycles pids,
    /// so a stale file can name a pid that now belongs to a different process, and the start
    /// time makes that check exact.
    fn is_live(&self, pid: u32) -> bool {
        if !self.is_local() {
            return false;
        }
        let Some(recorded) = self.proc_start.as_ref().map(json_scalar) else {
            return false;
        };
        crate::proc::start_time(pid).is_some_and(|actual| actual == recorded)
    }

    /// Whether [`SessionFile::pid`] counts in the pid namespace of this process.
    ///
    /// A file that a container wrote, or that another machine wrote onto a shared home, names a
    /// pid that belongs to a stranger here. `procStart` alone does not catch that: this tool
    /// compares the start time of the stranger against a value it did not write, and two
    /// processes that started in the same clock tick agree.
    ///
    /// A file without the key, and a machine that cannot say which domain it is, are both
    /// accepted. Both are the state before this key existed, and hiding every agent is worse
    /// than the collision this guards against.
    fn is_local(&self) -> bool {
        is_local_domain(self.pid_domain.as_deref(), crate::proc::local_pid_domain())
    }

    /// Milliseconds since the epoch that the current status was set, if it is known at all.
    fn status_set_at(&self) -> Option<i64> {
        self.status_updated_at
            .or(self.updated_at)
            .or(self.started_at)
    }

    /// The agent for this file, or `None` if the process behind it is gone.
    pub fn agent(&self, now_secs: i64) -> Option<Agent> {
        let pid = self.pid?;
        if !self.is_live(pid) {
            return None;
        }
        Some(Agent {
            status: self.status.clone(),
            status_age: status_age_secs(now_secs, self.status_set_at()),
            zellij: Zellij::of(pid),
            name: self.name.clone(),
            name_source: self.name_source.clone(),
            pid,
            session_id: self.session_id.clone(),
            session_started_at: epoch_secs(self.started_at),
            cwd: self.cwd.clone(),
            permission_mode: crate::proc::permission_mode(pid),
        })
    }
}

/// Whether a recorded pid domain is the domain of this process.
///
/// The two reads of the environment are arguments, so that the decision is testable on a machine
/// that cannot say which domain it is. A build sandbox without an `/etc/machine-id` is one.
fn is_local_domain(recorded: Option<&str>, local: Option<&str>) -> bool {
    match (recorded, local) {
        (Some(recorded), Some(local)) => recorded == local,
        _ => true,
    }
}

/// A JSON scalar as the shortest string that round-trips it, so a `procStart` of `987654` and a
/// `procStart` of `"987654"` compare equal.
fn json_scalar(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Number(n) => n.to_string(),
        other => other.to_string(),
    }
}

/// Whole seconds in the current status.
///
/// An unknown timestamp gives a status age of `0`. A missing field read as epoch millisecond `0`
/// would show every agent in its status for approximately 57 years, which looks like data and not
/// like a fault.
pub fn status_age_secs(now_secs: i64, status_set_at_ms: Option<i64>) -> u64 {
    let Some(set_at_ms) = status_set_at_ms else {
        return 0;
    };
    let elapsed_ms = now_secs.saturating_mul(1000).saturating_sub(set_at_ms);
    u64::try_from(elapsed_ms / 1000).unwrap_or(0)
}

/// Epoch seconds for a timestamp that Claude Code writes in milliseconds.
///
/// An absent timestamp gives `0`, for the same reason as [`status_age_secs`]. A consumer that
/// hides new sessions then computes a very large session age, so it hides nothing.
pub fn epoch_secs(ms: Option<i64>) -> u64 {
    let Some(ms) = ms else {
        return 0;
    };
    u64::try_from(ms / 1000).unwrap_or(0)
}

/// Where an agent runs in zellij.
///
/// The two fields are one object or nothing, and never one of the two. A session without a pane
/// is not an address that a consumer can use.
#[derive(serde::Serialize, Debug, Clone, PartialEq, Eq)]
pub struct Zellij {
    pub session: String,
    pub pane: String,
}

impl Zellij {
    /// Read from the environment of the agent. The session file gives the status of an agent
    /// and says nothing about zellij.
    fn of(pid: u32) -> Option<Self> {
        match crate::proc::zellij_of(pid) {
            (Some(session), Some(pane)) => Some(Zellij { session, pane }),
            _ => None,
        }
    }
}

/// Agent information that is printed to stdout.
#[derive(serde::Serialize, Debug, Clone, PartialEq, Eq)]
pub struct Agent {
    pub status: Option<String>,
    pub status_age: u64,
    pub zellij: Option<Zellij>,
    pub name: Option<String>,
    pub name_source: Option<String>,
    pub pid: u32,
    pub session_id: Option<String>,
    pub session_started_at: u64,
    pub cwd: Option<String>,
    /// The permission mode that the command line of the agent asks for, and `None` for a
    /// command line that does not ask for one.
    ///
    /// This is the launch of the agent, and not the mode it runs under now. The command line
    /// of a process does not change, and a person cycles the mode during a session.
    pub permission_mode: Option<String>,
}

#[cfg(test)]
mod tests {
    fn agent() -> super::Agent {
        super::Agent {
            status: Some("waiting".into()),
            status_age: 35,
            zellij: Some(super::Zellij {
                session: "work".into(),
                pane: "1".into(),
            }),
            name: Some("work-f8".into()),
            name_source: Some("user".into()),
            pid: 4242,
            session_id: Some("abc-123".into()),
            session_started_at: 1_755_000_000,
            cwd: Some("/home/you/src".into()),
            permission_mode: Some("plan".into()),
        }
    }

    #[test]
    fn serialises_every_key_in_the_published_order() {
        let json = serde_json::to_string(&agent()).unwrap();
        assert_eq!(
            json,
            r#"{"status":"waiting","status_age":35,"zellij":{"session":"work","pane":"1"},"name":"work-f8","name_source":"user","pid":4242,"session_id":"abc-123","session_started_at":1755000000,"cwd":"/home/you/src","permission_mode":"plan"}"#
        );
    }

    #[test]
    fn an_agent_outside_zellij_is_one_null() {
        let mut agent = agent();
        agent.zellij = None;
        let value: serde_json::Value = serde_json::to_value(&agent).unwrap();
        assert_eq!(value["zellij"], serde_json::Value::Null);
    }

    #[test]
    fn a_cwd_with_whitespace_needs_no_special_handling() {
        let mut agent = agent();
        agent.cwd = Some("/home/you/my projects/thing".into());
        let value: serde_json::Value = serde_json::to_value(&agent).unwrap();
        assert_eq!(value["cwd"], "/home/you/my projects/thing");
    }

    #[test]
    fn the_session_start_separates_a_newborn_from_a_finished_turn() {
        let newborn: super::SessionFile =
            serde_json::from_str(r#"{"startedAt":1755000000000,"statusUpdatedAt":1755000002000}"#)
                .unwrap();
        let finished: super::SessionFile =
            serde_json::from_str(r#"{"startedAt":1754000000000,"statusUpdatedAt":1755000002000}"#)
                .unwrap();

        // Identical from `status_age` alone -- the only thing a consumer had before this key.
        assert_eq!(
            super::status_age_secs(1_755_000_012, newborn.status_set_at()),
            super::status_age_secs(1_755_000_012, finished.status_set_at())
        );

        // Separable once the launch time is carried too.
        assert_eq!(super::epoch_secs(newborn.started_at), 1_755_000_000);
        assert_eq!(super::epoch_secs(finished.started_at), 1_754_000_000);
    }

    #[test]
    fn an_undated_session_start_is_zero_not_now() {
        assert_eq!(super::epoch_secs(None), 0);
        assert_eq!(super::epoch_secs(Some(-1)), 0);
        assert_eq!(super::epoch_secs(Some(1_755_000_000_999)), 1_755_000_000);
    }

    #[test]
    fn the_status_age_is_whole_seconds_since_the_status_was_set() {
        assert_eq!(super::status_age_secs(1_000, Some(940_500)), 59);
    }

    #[test]
    fn the_status_age_clamps_a_future_timestamp_to_zero() {
        assert_eq!(super::status_age_secs(1_000, Some(9_999_000)), 0);
    }

    #[test]
    fn the_status_age_of_an_undated_status_is_zero_not_the_epoch() {
        assert_eq!(super::status_age_secs(1_755_000_000, None), 0);
    }

    #[test]
    fn proc_start_compares_equal_as_number_or_string() {
        assert_eq!(super::json_scalar(&serde_json::json!(987654)), "987654");
        assert_eq!(super::json_scalar(&serde_json::json!("987654")), "987654");
    }

    #[test]
    fn session_file_tolerates_a_moving_schema() {
        let file: super::SessionFile =
            serde_json::from_str(r#"{"pid":7,"status":"shell","somethingNew":true,"cwd":"/tmp"}"#)
                .expect("unknown fields must not be an error");
        assert_eq!(file.pid, Some(7));
        assert_eq!(file.status.as_deref(), Some("shell"));
        assert_eq!(file.name, None);
        assert_eq!(file.status_set_at(), None);
    }

    #[test]
    fn status_set_at_prefers_the_most_specific_stamp() {
        let file: super::SessionFile =
            serde_json::from_str(r#"{"statusUpdatedAt":3,"updatedAt":2,"startedAt":1}"#).unwrap();
        assert_eq!(file.status_set_at(), Some(3));

        let file: super::SessionFile = serde_json::from_str(r#"{"startedAt":1}"#).unwrap();
        assert_eq!(file.status_set_at(), Some(1));
    }

    #[test]
    fn a_file_without_proc_start_is_not_live() {
        let file: super::SessionFile = serde_json::from_str(r#"{"pid":1}"#).unwrap();
        assert!(file.agent(0).is_none());
    }

    /// The pid of a file from a container, or from another machine on a shared home, is a name
    /// in a namespace that is not this one. It is never looked up here.
    #[test]
    fn a_pid_from_another_domain_is_not_local() {
        assert!(!super::is_local_domain(
            Some("linux:0123:pid:[1]"),
            Some("linux:4567:pid:[4026531836]")
        ));
    }

    #[test]
    fn a_pid_from_this_domain_is_local() {
        let domain = "linux:0123:pid:[4026531836]";
        assert!(super::is_local_domain(Some(domain), Some(domain)));
    }

    /// The state before the key existed. Rejecting these files hides every agent of an older
    /// Claude Code, which is worse than the collision the key guards against.
    #[test]
    fn a_file_without_a_domain_is_accepted() {
        assert!(super::is_local_domain(None, Some("linux:0123:pid:[1]")));

        let file: super::SessionFile = serde_json::from_str(r#"{"pid":1}"#).unwrap();
        assert!(file.is_local());
    }

    /// A build sandbox without an `/etc/machine-id` is such a machine, and every agent of a
    /// person who runs the tool there would go away.
    #[test]
    fn a_machine_that_cannot_say_its_domain_accepts_every_file() {
        assert!(super::is_local_domain(Some("linux:0123:pid:[1]"), None));
        assert!(super::is_local_domain(None, None));
    }

    /// A `derived` name is the basename of the cwd and a suffix, and a `user` name is a label
    /// that a person chose. A consumer that shows the cwd anyway needs the two apart.
    #[test]
    fn the_name_source_is_carried_through_verbatim() {
        for source in [
            "user",
            "peer",
            "derived",
            "collision",
            "auto",
            "hook",
            "somethingNew",
        ] {
            let file: super::SessionFile =
                serde_json::from_str(&format!(r#"{{"nameSource":"{source}"}}"#)).unwrap();
            assert_eq!(file.name_source.as_deref(), Some(source));
        }

        let file: super::SessionFile = serde_json::from_str(r#"{"name":"x"}"#).unwrap();
        assert_eq!(file.name_source, None);
    }
}
