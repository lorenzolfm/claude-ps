# claude-ps

`claude-ps` is `ps` for Claude Code. It prints all running agents as JSON, or
as a table with `--format text`.

```console
$ claude-ps --format text
=======================================================================================
 NAME        STATUS   AGE  ELAPSED  MODE               CWD                  PID  ZELLIJ
=======================================================================================
 scratch-2c  busy     13s    2h 5m  bypassPermissions  ~/scratch        3129839  -
 work-f8~    waiting  35s    1h 4m  -                  ~/Projects/work  3318865  work:0
=======================================================================================
```

## Installation

### Cargo

```sh
cargo build --release
ln -sf "$PWD/target/release/claude-ps" ~/.local/bin/claude-ps
```

### Nix

```sh
nix profile install github:lorenzolfm/claude-ps
```

## Requirements

Linux only. `claude-ps` reads `/proc/<pid>/environ`, `/proc/<pid>/stat`, and
`/proc/<pid>/cmdline`.
There is no procfs on darwin, so the flake does not build for darwin.

## Output

`claude-ps` writes a JSON array to stdout. Each object is one agent.

| Key | Content |
|---|---|
| `status` | What Claude reports, verbatim |
| `status_age` | Whole seconds in that status |
| `zellij` | `{session, pane}`, or `null` if the agent is not in zellij. A variable that is set and empty carries no address, so it is `null` too |
| `name` | Claude's own label for the session |
| `name_source` | Who chose the name, or `null` |
| `pid` | The process id |
| `session_id` | Claude's session uuid, which is also the transcript name |
| `session_started_at` | Epoch seconds when the session started, or `0` if unknown |
| `cwd` | The working directory of the agent |
| `permission_mode` | The mode the command line asks for, or `null` |

```console
$ claude-ps
[
  {
    "status": "waiting",
    "status_age": 35,
    "zellij": { "session": "work", "pane": "0" },
    "name": "work-f8",
    "name_source": "derived",
    "pid": 3318865,
    "session_id": "b08aacbc-…",
    "session_started_at": 1755000000,
    "cwd": "/home/you/Projects/work",
    "permission_mode": null
  },
  {
    "status": "busy",
    "status_age": 13,
    "zellij": null,
    "name": "scratch-2c",
    "name_source": "user",
    "pid": 3129839,
    "session_id": "52b7681e-…",
    "session_started_at": 1755004000,
    "cwd": "/home/you/scratch",
    "permission_mode": "bypassPermissions"
  }
]
```

## A table for a person

`claude-ps --format text` prints the same agents as the table at the top of
this page. It shows the durations in hours and minutes, the home directory as
`~`, and the zellij address as `session:pane`. A key that the session file does
not have is `-`.

`AGE` is the time in the current status. `ELAPSED` is the time since the
session started. A `~` after a name marks a name that Claude derived rather
than a person chose, because such a name repeats the `CWD` on the same line.

This table is for eyes only. The columns, the order, and the format of a value
change without a note, and no agents prints `no agents` and not an empty
document. Read the JSON from a program.

## Rules for consumers

**The status vocabulary is open.** Claude Code adds new values in new releases.
`claude-ps` passes the status through without a change. Do not compare the
status against a fixed set of values. A consumer that shows only the values it
knows hides live agents. `name_source` and `permission_mode` are open in the
same way.

**`name_source` says whether the name carries information.** A `derived` name is
the basename of the `cwd` and a suffix, so a consumer that already shows the
`cwd` shows it twice. Claude Code writes `user`, `peer`, `derived`, `collision`,
`auto`, and `hook`. Of those, `user` and `peer` are the names that a person or
another agent chose. Show the `cwd` for every agent, and add the name when the
source is one of those two, or when it is absent, which is the state before this
key existed. Suppress a source you do not know rather than trusting it: the
sources that carry a name are the short list, and the machinery is the long one.

**`permission_mode` is the launch, and not the mode now.** It comes from the
command line of the agent, which does not change. A person who cycles the mode
during a session does not move this key, and an agent that was launched with no
flag reports `null` and not the mode it fell back to. It answers what an agent
was started with. It does not answer what an agent may do right now.

**`status_age` and `session_started_at` answer different questions.**
`status_age` is the time in the current status. `session_started_at` is the time
when the session started. A new session and a session that completed a turn are
both `idle` with a small `status_age`. Only `session_started_at` makes them
different.

**No agents is `[]`.** The output is always a JSON document, also when nothing
runs.

**Stale sessions do not appear.** Claude Code keeps the session file after the
agent stops. `claude-ps` shows an agent only if the pid belongs to this machine
and this pid namespace, the pid is alive, and the start time of the process
agrees with the session file. A session file that a container wrote, or that
another machine wrote onto a shared home, names a pid that means nothing here,
and this tool never looks it up.

**The exit status is 0 after a successful write.** It is also 0 if the reader
closes the pipe, which is what `head` does. It is 1 if `claude-ps` cannot write
its output, for example when the disk is full, and the message goes to stderr.

**The order is stable, and not presentational.** `claude-ps` sorts the agents by
pid. Sort the agents again if you show them to a person. The table of
`--format text` is sorted by name.

## Zellij support

If an agent runs in zellij, `claude-ps` reports the zellij session and pane in
the `zellij` key. The value is `null` for an agent outside zellij, and for one
whose `ZELLIJ_SESSION_NAME` or `ZELLIJ_PANE_ID` is set and empty: a variable
with nothing in it carries no address to attach to.

[luneta](https://github.com/lorenzolfm/luneta) uses this key. Its agent screen
shows all running agents. `Enter` attaches to the session and focuses the pane.

If you use luneta, install `claude-ps` on the `PATH` of the zellij **server**,
which is not the `PATH` of your shell. A server that started before the
installation does not find the tool. Restart the server, or set the
`agents_command` configuration key of luneta to the path of the executable.

## Other consumers

- [claude-tray](https://github.com/lorenzolfm/claude-tray) — a system tray
  applet that shows which sessions wait for you.

## License

MIT
