mod agent;
mod human;
mod proc;

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
  zellij              {session, pane}, or null if the agent is not in zellij
  name                Claude's own label for the session
  pid                 the process id
  session_id          Claude's session uuid, which is also the transcript name
  session_started_at  epoch seconds when the session started, or 0 if unknown
  cwd                 the working directory of the agent

The status vocabulary is open and changes with the version of Claude Code. Do
not compare the status against a fixed set of values.

status_age and session_started_at answer different questions. status_age is the
time in the current status. session_started_at is the time when the session
started. A new session and a session that completed a turn are both idle with a
small status_age. Only session_started_at makes them different.

Agents with a stale session file do not appear: the pid must be alive, and the
start time of the process must agree with the session file.

The order is by pid. This order is for stable diffs. Sort the agents again to
show them to a person.

--format text prints a table for a person instead: one line for each agent, the
timestamps as durations, and the home directory as ~. That table has no
stability rule, and its order is by name. Read the JSON from a program."
)]
struct Cli {
    /// json for a program, text for a person
    #[arg(short, long, value_enum, default_value_t = Format::Json)]
    format: Format,
}

#[derive(clap::ValueEnum, Clone, Copy, PartialEq, Eq, Debug)]
enum Format {
    /// A JSON array, one object for each agent. The documented format.
    Json,
    /// A padded table with one header line. For eyes only.
    Text,
}

fn main() -> std::process::ExitCode {
    let cli = Cli::parse();

    match run(cli.format) {
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

fn run(format: Format) -> Result<String, String> {
    let now_secs = i64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| "the system clock is before the epoch".to_string())?
            .as_secs(),
    )
    .map_err(|_| "the system clock is too far in the future".to_string())?;

    let home = home()?;
    let mut agents = collect(&sessions_dir(&home), now_secs);

    // A stable order, so that two runs give a small diff. The pid is unique and does not
    // change while the agent runs.
    agents.sort_by_key(|agent| agent.pid);

    match format {
        Format::Json => {
            // One key per line, so two runs one second apart give a small diff.
            let mut out = serde_json::to_string_pretty(&agents)
                .map_err(|error| format!("could not serialise the agent list: {error}"))?;
            out.push('\n');
            Ok(out)
        }
        Format::Text => Ok(human::table(&agents, now_secs, &home)),
    }
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
fn collect(dir: &std::path::PathBuf, now_secs: i64) -> Vec<agent::Agent> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .filter_map(|path| std::fs::read_to_string(&path).ok())
        .filter_map(|text| serde_json::from_str::<agent::SessionFile>(&text).ok())
        .filter_map(|file| file.agent(now_secs))
        .collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_missing_sessions_directory_yields_no_agents() {
        let agents = super::collect(&std::path::PathBuf::from("/nonexistent/claude/sessions"), 0);
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
