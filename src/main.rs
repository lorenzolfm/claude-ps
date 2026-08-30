mod agent;
mod proc;
mod transcript;

use clap::Parser;
use std::io::Write;

#[derive(clap::Parser)]
#[command(
    name = "claude-ps",
    version,
    about = "ps for Claude Code: all running agents, as JSON",
    long_about = "\
ps for Claude Code. Prints all running agents as a JSON array on stdout, one
object per agent:

  status              what Claude reports, verbatim (busy, idle, waiting, ...)
  status_age          whole seconds in that status
  context             {tokens, as_of} at the last assistant turn, or null
  zellij              {session, pane}, or null if the agent is not in zellij
  name                Claude's own label for the session
  pid                 the process id
  session_id          Claude's session uuid, which is also the transcript name
  session_started_at  epoch seconds when the session started, or 0 if unknown
  cwd                 the working directory of the agent

The status vocabulary is open and changes with the version of Claude Code. Do
not compare the status against a fixed set of values.

context is a token count, and not a percentage: Claude Code does not write the
size of the context window to disk. The as_of stamp gives the time of the last
completed assistant turn.

status_age and session_started_at answer different questions. status_age is the
time in the current status. session_started_at is the time when the session
started. A new session and a session that completed a turn are both idle with a
small status_age. Only session_started_at makes them different.

Agents with a stale session file do not appear: the pid must be alive, and the
start time of the process must agree with the session file.

The order is by zellij session, then pane, then pid, with agents outside zellij
last. This order is for stable diffs. Sort the agents again to show them to a
person."
)]
struct Cli {}

fn main() -> std::process::ExitCode {
    Cli::parse();

    match run() {
        Ok(output) => match write_stdout(&output) {
            Ok(()) => std::process::ExitCode::SUCCESS,
            // The reader closed the pipe, which is what `| head` does. The reader has the
            // data it wants, so this tool did not fail.
            Err(error) if error.kind() == std::io::ErrorKind::BrokenPipe => {
                std::process::ExitCode::SUCCESS
            }
            // All other write errors are real. A full disk during `claude-ps > agents.json`
            // gives a truncated file, and the caller must hear about it.
            Err(error) => {
                eprintln!("claude-ps: could not write to stdout: {error}");
                std::process::ExitCode::FAILURE
            }
        },
        Err(error) => {
            eprintln!("claude-ps: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}

/// Write the document to stdout, then flush it.
///
/// One call for the full document, so a consumer that reads this on a timer sees all of it or
/// none of it.
///
/// The flush is explicit. `Stdout` is line buffered, so a write can report success and the
/// flush that follows can still fail. The runtime flushes at exit and discards that error.
fn write_stdout(output: &str) -> std::io::Result<()> {
    let mut stdout = std::io::stdout();
    stdout.write_all(output.as_bytes())?;
    stdout.flush()
}

fn run() -> Result<String, String> {
    let now_secs = i64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| "the system clock is before the epoch".to_string())?
            .as_secs(),
    )
    .map_err(|_| "the system clock is too far in the future".to_string())?;

    let home = home()?;
    let mut agents = collect(&sessions_dir(&home), now_secs, &home);
    agents.sort_by(|a, b| a.sort_key().cmp(&b.sort_key()));

    // One key per line, so two runs one second apart give a small diff.
    let mut out = serde_json::to_string_pretty(&agents)
        .map_err(|error| format!("could not serialise the agent list: {error}"))?;
    out.push('\n');
    Ok(out)
}

fn home() -> Result<String, String> {
    std::env::var("HOME").map_err(|_| "HOME is not set".to_string())
}

fn sessions_dir(home: &str) -> std::path::PathBuf {
    std::path::PathBuf::from(home)
        .join(".claude")
        .join("sessions")
}

/// All readable session files that have a live agent.
///
/// A file that this tool cannot read or parse is skipped without a message. Claude Code writes
/// to this directory while this tool reads it, so an incomplete file is a normal event.
fn collect(dir: &std::path::PathBuf, now_secs: i64, home: &str) -> Vec<agent::Agent> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .filter_map(|path| std::fs::read_to_string(&path).ok())
        .filter_map(|text| serde_json::from_str::<agent::SessionFile>(&text).ok())
        .filter_map(|file| file.agent(now_secs, home))
        .collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_missing_sessions_directory_yields_no_agents() {
        let agents = super::collect(
            &std::path::PathBuf::from("/nonexistent/claude/sessions"),
            0,
            "/home/you",
        );
        assert!(agents.is_empty());
    }

    #[test]
    fn no_agents_is_an_empty_array_not_empty_output() {
        let empty: Vec<crate::agent::Agent> = Vec::new();
        assert_eq!(serde_json::to_string(&empty).unwrap(), "[]");
    }

    #[test]
    fn the_cli_definition_is_valid() {
        use clap::CommandFactory;

        super::Cli::command().debug_assert();
    }
}
