//! `claude-ps` — one line per running Claude Code agent, joined to its zellij pane.
//!
//! Claude Code writes `~/.claude/sessions/<pid>.json` for each running agent, carrying what it
//! is doing. That file says nothing about zellij. The agent's own environment says nothing
//! about what it is doing. `pid` is the only thing they share, and joining on it is the whole
//! job of this tool.

mod agent;
mod proc;

use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};

use clap::Parser;

use agent::{Row, SessionFile};

/// Printed by `--help`, which the Python this replaces did not implement at all: it ignored
/// `argv` entirely and printed the table whatever you asked it.
#[derive(Parser)]
#[command(
    name = "claude-ps",
    version,
    about = "One line per running Claude Code agent, joined to its zellij pane",
    long_about = "\
One line per running Claude Code agent, joined to the zellij pane it runs in.

Output is TAB-separated, one agent per line, in a fixed column order:

    status  age  session  pane  name  pid  session_id  started_at  cwd

  status      whatever Claude reports, verbatim (busy, idle, waiting, shell, ...)
  age         whole seconds spent in that status
  session     ZELLIJ_SESSION_NAME, or - if the agent is not in zellij
  pane        ZELLIJ_PANE_ID, or - likewise
  name        Claude's own derived name, NOT the zellij session name
  pid         the process id, and the key the two halves are joined on
  session_id  Claude's session uuid, matching its transcript
  started_at  epoch seconds the session began, or 0 if unknown
  cwd         last, and the only field that may contain whitespace

age and started_at answer different questions and a consumer needs both. A
session that has just launched and one that has just finished a turn are both
idle with a small age; only the launch time tells them apart.

The status vocabulary is open and moves with Claude Code's version, so it is
passed through untouched. Do not match it against a fixed set.

Rows are sorted by session, then pane, then pid. That is for stable diffs
between runs, not for display: ordering for a human belongs to the consumer.

Agents whose session file is stale are omitted. Liveness is exact rather than
heuristic -- the pid must be alive AND have started when the file says it did,
so a recycled pid cannot pass a dead agent off as a live one."
)]
struct Cli {}

fn main() -> ExitCode {
    Cli::parse();
    match run() {
        Ok(output) => {
            // Written in one call: a consumer polling this on a timer should see a whole table
            // or nothing, never half of one.
            if io::stdout().write_all(output.as_bytes()).is_err() {
                // A closed pipe is what `| head` looks like from in here. Not an error.
                return ExitCode::SUCCESS;
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("claude-ps: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<String, String> {
    let now_secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| "the system clock is before the epoch".to_string())?
        .as_secs_f64();

    let mut rows = collect(&sessions_dir()?, now_secs);
    rows.sort_by(|a, b| a.sort_key().cmp(&b.sort_key()));

    let mut out = String::new();
    for row in &rows {
        out.push_str(&row.to_tsv());
        out.push('\n');
    }
    Ok(out)
}

fn sessions_dir() -> Result<PathBuf, String> {
    let home = std::env::var_os("HOME").ok_or("HOME is not set")?;
    Ok(PathBuf::from(home).join(".claude").join("sessions"))
}

/// Every readable session file that still has a live agent behind it.
///
/// ⚠️ Unreadable and malformed files are skipped in silence, and that is deliberate. This
/// directory is written by another program while this one reads it, so a half-written file is
/// an ordinary event rather than a fault — and the caller is usually a status bar or a picker,
/// which cannot do anything useful with a complaint about one file.
fn collect(dir: &PathBuf, now_secs: f64) -> Vec<Row> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .filter_map(|path| fs::read_to_string(&path).ok())
        .filter_map(|text| serde_json::from_str::<SessionFile>(&text).ok())
        .filter_map(|file| file.row(now_secs))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A missing sessions directory means no agents, not a crash. This is what a machine that
    /// has never run Claude Code looks like.
    #[test]
    fn a_missing_sessions_directory_yields_no_rows() {
        let rows = collect(&PathBuf::from("/nonexistent/claude/sessions"), 0.0);
        assert!(rows.is_empty());
    }

    /// `--help` and `--version` must exist. The predecessor's lack of them is the defect this
    /// binary was written to stop repeating.
    #[test]
    fn the_cli_definition_is_valid() {
        use clap::CommandFactory;
        Cli::command().debug_assert();
    }
}
