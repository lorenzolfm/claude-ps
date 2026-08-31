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
    /// The raw read. The vocabulary is open and changes with the version of Claude Code, so this
    /// tool does not compare the status against a known set of values. [`Text`] is what leaves
    /// this tool.
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
            status: Text::word(self.status.as_deref()),
            status_age: status_age_secs(now_secs, self.status_set_at()),
            zellij: Zellij::of(pid),
            name: Name::of(self.name.as_deref(), self.name_source.as_deref()),
            pid,
            session_id: Text::verbatim(self.session_id.as_deref()),
            session_started_at: epoch_secs(self.started_at),
            cwd: Text::verbatim(self.cwd.as_deref()),
            permission_mode: Text::word(crate::proc::permission_mode(pid).as_deref()),
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

/// Whole seconds in the current status, and `None` for a status that no timestamp dates.
///
/// A status set in this second is `Some(0)`, and the two are not the same answer. The `0` of the
/// JSON spells both, which is the one thing this tool cannot change; inside it they stay apart,
/// and the table prints the missing mark for the one that is not a duration.
///
/// A missing field read as epoch millisecond `0` would show every agent in its status for
/// approximately 57 years, which looks like data and not like a fault.
pub fn status_age_secs(now_secs: i64, status_set_at_ms: Option<i64>) -> Option<u64> {
    let set_at_ms = status_set_at_ms?;
    let elapsed_ms = now_secs.saturating_mul(1000).saturating_sub(set_at_ms);
    // A status that a clock change dates in the future is `0` seconds old, which is an answer.
    Some(u64::try_from(elapsed_ms / 1000).unwrap_or(0))
}

/// Epoch seconds for a timestamp that Claude Code writes in milliseconds, and `None` for a
/// session that carries no start.
///
/// A start before the epoch is a start that no clock wrote, and it is unknown for the same
/// reason. A consumer that hides new sessions would otherwise compute a session age of the age
/// of the epoch, so it hides nothing.
pub fn epoch_secs(ms: Option<i64>) -> Option<u64> {
    Some(u64::try_from(ms?).ok()? / 1000)
}

/// `0` is the published spelling of an unknown timestamp, so the encoding lives here and nowhere
/// else. The JSON is a contract, and `status_age` and `session_started_at` have both spelled an
/// absence that way since the first release.
fn zero_when_unknown<S: serde::Serializer>(
    secs: &Option<u64>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    serializer.serialize_u64(secs.unwrap_or(0))
}

/// Where an agent runs in zellij.
///
/// The two fields are one object or nothing, and never one of the two. A session without a pane
/// is not an address that a consumer can use.
#[derive(serde::Serialize, Debug, Clone, PartialEq, Eq)]
pub struct Zellij {
    pub session: Text,
    pub pane: Text,
}

impl Zellij {
    /// Read from the environment of the agent. The session file gives the status of an agent
    /// and says nothing about zellij.
    fn of(pid: u32) -> Option<Self> {
        Self::address(crate::proc::zellij_of(pid))
    }

    /// The address in a pair of environment values, or `None` when a half of it is missing.
    ///
    /// A half that is empty is a half that is missing. `ZELLIJ_SESSION_NAME=` is a variable that
    /// carries no session, and an address with nothing in it is one that no consumer can attach
    /// to. A session name is [`Text::verbatim`] and not a word: the space in it is part of the
    /// name that `zellij attach` wants.
    ///
    /// Separate from the read, because an environment is difficult to make on a real process and
    /// this decision is not.
    pub(crate) fn address(vars: (Option<String>, Option<String>)) -> Option<Self> {
        let (session, pane) = vars;
        Some(Zellij {
            session: Text::verbatim(session.as_deref())?,
            pane: Text::verbatim(pane.as_deref())?,
        })
    }
}

/// The label of a session, and who chose it.
///
/// The same decision as [`Zellij`]: one object or nothing, and never one of the two. A source
/// says whether the name carries information, and it has nothing to say about a name that is
/// not there. The two keys stay two keys on the wire.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Name {
    pub text: Text,
    /// Who chose the name: `user`, `peer`, `derived`, and the ones a later release adds. Absent
    /// is the state before Claude Code wrote this key, and it reads as a name that was chosen.
    pub source: Option<Text>,
}

impl Name {
    /// The name in a session file, or `None` for a file that carries none. A source without a
    /// name goes with the name it describes.
    pub fn of(name: Option<&str>, source: Option<&str>) -> Option<Self> {
        Some(Name {
            text: Text::verbatim(name)?,
            source: Text::word(source),
        })
    }
}

/// `name` and `name_source` are two keys of the published object, and this is the one place that
/// knows they are one value. A session without a name writes both as `null`, which is what the
/// object carried before the two became one field.
fn name_and_source<S: serde::Serializer>(
    name: &Option<Name>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    use serde::ser::SerializeMap;

    let mut map = serializer.serialize_map(Some(2))?;
    map.serialize_entry("name", &name.as_ref().map(|name| &name.text))?;
    map.serialize_entry(
        "name_source",
        &name.as_ref().and_then(|name| name.source.as_ref()),
    )?;
    map.end()
}

/// A value that a foreign source wrote: present, never empty, and otherwise verbatim.
///
/// Absence is `None`. An empty string is an absence that reads as data: it leaves as `""` in the
/// JSON and as a blank cell in the table, and every consumer then handles a second spelling of
/// nothing. The session file belongs to Claude Code and changes with its releases, so a key that
/// carries a value today can carry an empty one tomorrow.
///
/// Two constructors, because the two kinds of value differ in one thing only. Both refuse
/// emptiness, and only a word is trimmed.
#[derive(serde::Serialize, Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
#[serde(transparent)]
pub struct Text(String);

impl Text {
    /// A word from an open vocabulary: a status, a name source, a permission mode. The space
    /// around such a word is not part of it.
    pub fn word(raw: Option<&str>) -> Option<Self> {
        raw.map(str::trim)
            .filter(|word| !word.is_empty())
            .map(|word| Self(word.to_string()))
    }

    /// A name, a path, an identifier: something that somebody else chose. Only emptiness goes.
    /// A directory name can end in a space, and a trim names a different directory.
    pub fn verbatim(raw: Option<&str>) -> Option<Self> {
        raw.filter(|value| !value.is_empty())
            .map(|value| Self(value.to_string()))
    }
}

impl std::ops::Deref for Text {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// Agent information that is printed to stdout.
#[derive(serde::Serialize, Debug, Clone, PartialEq, Eq)]
pub struct Agent {
    pub status: Option<Text>,
    /// Whole seconds in the current status, and `None` for a status that no timestamp dates.
    /// Both leave as a number, because the `0` of an unknown age is the published spelling.
    #[serde(serialize_with = "zero_when_unknown")]
    pub status_age: Option<u64>,
    pub zellij: Option<Zellij>,
    #[serde(flatten, serialize_with = "name_and_source")]
    pub name: Option<Name>,
    pub pid: u32,
    pub session_id: Option<Text>,
    #[serde(serialize_with = "zero_when_unknown")]
    pub session_started_at: Option<u64>,
    pub cwd: Option<Text>,
    /// The permission mode that the command line of the agent asks for, and `None` for a
    /// command line that does not ask for one.
    ///
    /// This is the launch of the agent, and not the mode it runs under now. The command line
    /// of a process does not change, and a person cycles the mode during a session.
    pub permission_mode: Option<Text>,
}

#[cfg(test)]
mod tests {
    fn agent() -> super::Agent {
        super::Agent {
            status: super::Text::word(Some("waiting")),
            status_age: Some(35),
            zellij: address("work", "1"),
            name: super::Name::of(Some("work-f8"), Some("user")),
            pid: 4242,
            session_id: super::Text::verbatim(Some("abc-123")),
            session_started_at: Some(1_755_000_000),
            cwd: super::Text::verbatim(Some("/home/you/src")),
            permission_mode: super::Text::word(Some("plan")),
        }
    }

    fn address(session: &str, pane: &str) -> Option<super::Zellij> {
        super::Zellij::address((Some(session.to_string()), Some(pane.to_string())))
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

    /// Anything that launches an agent can set `ZELLIJ_SESSION_NAME=` -- a wrapper script, a
    /// systemd unit -- and a variable that is present carries no address on its own. Luneta
    /// reads this key to attach, and `:` is a pane of no session.
    #[test]
    fn a_half_of_an_address_with_nothing_in_it_is_a_half_that_is_missing() {
        assert_eq!(address("", "1"), None);
        assert_eq!(address("work", ""), None);
        assert_eq!(address("", ""), None);
        assert_eq!(super::Zellij::address((None, None)), None);
    }

    #[test]
    fn a_whole_address_survives() {
        let zellij = address("work", "1").expect("an address");
        assert_eq!(&*zellij.session, "work");
        assert_eq!(&*zellij.pane, "1");
    }

    /// A session name is an identifier that a person chose, and not a word from a vocabulary.
    /// Trimming it names a different session, and `zellij attach` then finds nothing.
    #[test]
    fn a_session_name_keeps_the_space_that_is_part_of_it() {
        let zellij = address(" my work ", " 1 ").expect("an address");
        assert_eq!(&*zellij.session, " my work ");
        assert_eq!(&*zellij.pane, " 1 ");
    }

    #[test]
    fn an_agent_whose_address_was_empty_is_one_null_too() {
        let mut agent = agent();
        agent.zellij = address("", "");
        let value: serde_json::Value = serde_json::to_value(&agent).unwrap();
        assert_eq!(value["zellij"], serde_json::Value::Null);
    }

    #[test]
    fn a_cwd_with_whitespace_needs_no_special_handling() {
        let mut agent = agent();
        agent.cwd = super::Text::verbatim(Some("/home/you/my projects/thing"));
        let value: serde_json::Value = serde_json::to_value(&agent).unwrap();
        assert_eq!(value["cwd"], "/home/you/my projects/thing");
    }

    /// Claude Code writes `status` as a free string, and a string with nothing in it is not a
    /// status. It leaves as `null`, which is the absence every consumer already handles.
    #[test]
    fn a_status_with_nothing_in_it_is_absent_and_not_an_empty_word() {
        for raw in ["", " ", "\t", "\n  "] {
            assert_eq!(super::Text::word(Some(raw)), None);
        }
        assert_eq!(super::Text::word(None), None);

        let mut agent = agent();
        agent.status = super::Text::word(Some("  "));
        let value: serde_json::Value = serde_json::to_value(&agent).unwrap();
        assert_eq!(value["status"], serde_json::Value::Null);
    }

    /// A word is otherwise the word Claude wrote, including one this tool does not know. Only
    /// the space around it goes.
    #[test]
    fn a_status_word_is_carried_through_verbatim() {
        for raw in ["waiting", "idle", "busy", "shell", "somethingNew"] {
            let word = super::Text::word(Some(raw)).expect("a word");
            assert_eq!(&*word, raw);
            assert_eq!(serde_json::to_string(&word).unwrap(), format!(r#""{raw}""#));
        }
        assert_eq!(&*super::Text::word(Some(" busy ")).unwrap(), "busy");
    }

    /// The same release of Claude Code that can clear a status can clear a name, an id or a
    /// cwd. Every one of them leaves as `null`, which is the absence a consumer already reads.
    #[test]
    fn a_key_with_nothing_in_it_is_absent_for_all_of_them() {
        let mut agent = agent();
        agent.name = super::Name::of(Some(""), Some(""));
        agent.session_id = super::Text::verbatim(Some(""));
        agent.cwd = super::Text::verbatim(Some(""));
        agent.permission_mode = super::Text::word(Some(""));

        let value: serde_json::Value = serde_json::to_value(&agent).unwrap();
        for key in [
            "name",
            "name_source",
            "session_id",
            "cwd",
            "permission_mode",
        ] {
            assert_eq!(value[key], serde_json::Value::Null, "{key}");
        }
    }

    /// A path is not a word. `/home/you/two spaces /` is a directory that exists, and a trim
    /// names a different one, which a consumer then fails to open.
    #[test]
    fn a_word_is_trimmed_and_a_name_is_not() {
        assert_eq!(&*super::Text::word(Some("  plan  ")).unwrap(), "plan");
        assert_eq!(
            &*super::Text::verbatim(Some("/home/you/two spaces / ")).unwrap(),
            "/home/you/two spaces / "
        );
        assert_eq!(&*super::Text::verbatim(Some(" ")).unwrap(), " ");
    }

    /// The published JSON is the reason for `#[serde(transparent)]`: a value that was legal
    /// before this type existed is byte for byte the value it was.
    #[test]
    fn a_narrowed_value_is_the_same_on_the_wire() {
        let json = serde_json::to_string(&super::Text::verbatim(Some("work-f8"))).unwrap();
        assert_eq!(json, r#""work-f8""#);
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
        assert_eq!(super::epoch_secs(newborn.started_at), Some(1_755_000_000));
        assert_eq!(super::epoch_secs(finished.started_at), Some(1_754_000_000));
    }

    #[test]
    fn an_undated_session_start_is_absent_and_not_now() {
        assert_eq!(super::epoch_secs(None), None);
        assert_eq!(super::epoch_secs(Some(-1)), None);
        assert_eq!(
            super::epoch_secs(Some(1_755_000_000_999)),
            Some(1_755_000_000)
        );
    }

    #[test]
    fn the_status_age_is_whole_seconds_since_the_status_was_set() {
        assert_eq!(super::status_age_secs(1_000, Some(940_500)), Some(59));
    }

    #[test]
    fn the_status_age_clamps_a_future_timestamp_to_zero() {
        assert_eq!(super::status_age_secs(1_000, Some(9_999_000)), Some(0));
    }

    /// A status that no timestamp dates and a status that was set in this second both print
    /// `0` on the wire, and only one of them is a duration. The reader of the JSON cannot tell
    /// them apart, and nothing inside this tool has to guess which one it holds.
    #[test]
    fn an_undated_status_and_a_status_of_this_second_are_not_the_same_age() {
        assert_eq!(super::status_age_secs(1_755_000_000, None), None);
        assert_eq!(
            super::status_age_secs(1_755_000_000, Some(1_755_000_000_000)),
            Some(0)
        );
    }

    /// The `0` of an unknown timestamp is the published spelling, and a consumer reads it
    /// today. It stays a number, and it never becomes `null`.
    #[test]
    fn an_unknown_timestamp_is_zero_on_the_wire() {
        let mut agent = agent();
        agent.status_age = None;
        agent.session_started_at = None;
        let value: serde_json::Value = serde_json::to_value(&agent).unwrap();
        assert_eq!(value["status_age"], 0);
        assert_eq!(value["session_started_at"], 0);
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

    /// A source is a fact about a name. `{"name": null, "name_source": "derived"}` says who
    /// chose a name that no key carries, and a consumer that reads the source to decide whether
    /// to show the name has nothing to show either way.
    #[test]
    fn a_source_without_a_name_is_no_name_at_all() {
        assert_eq!(super::Name::of(None, Some("derived")), None);
        assert_eq!(super::Name::of(Some(""), Some("derived")), None);

        let mut agent = agent();
        agent.name = super::Name::of(None, Some("derived"));
        let value: serde_json::Value = serde_json::to_value(&agent).unwrap();
        assert_eq!(value["name"], serde_json::Value::Null);
        assert_eq!(value["name_source"], serde_json::Value::Null);
    }

    /// A name that Claude Code wrote before it wrote a source. It is a name a person chose,
    /// as far as anything here can say.
    #[test]
    fn a_name_without_a_source_is_still_a_name() {
        let name = super::Name::of(Some("work-f8"), None).expect("a name");
        assert_eq!(&*name.text, "work-f8");
        assert_eq!(name.source, None);
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
