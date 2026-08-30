# claude-ps

`claude-ps` prints one line for each running Claude Code agent. Each line
carries what the agent is doing and the zellij pane it is doing it in.

## The problem

Claude Code writes `~/.claude/sessions/<pid>.json` for each running agent. The
file carries the agent's status, its working directory, and the time the status
was set. It says nothing about zellij.

The agent's own environment carries `ZELLIJ_SESSION_NAME` and
`ZELLIJ_PANE_ID`. It says nothing about what the agent is doing.

The two halves share one thing: the process id. `claude-ps` joins them.

## Output

One agent per line, TAB-separated, in a fixed column order:

```
status  age  session  pane  name  pid  session_id  started_at  cwd
```

| Column | Content |
|---|---|
| `status` | whatever Claude reports, verbatim |
| `age` | whole seconds spent in that status |
| `session` | `ZELLIJ_SESSION_NAME`, or `-` if the agent is not in zellij |
| `pane` | `ZELLIJ_PANE_ID`, or `-` likewise |
| `name` | Claude's own derived name, **not** the zellij session name |
| `pid` | the process id, and the key the two halves are joined on |
| `session_id` | Claude's session uuid, matching its transcript |
| `started_at` | epoch **seconds** the session began, or `0` if unknown |
| `cwd` | last, and the only field that may contain whitespace |

```console
$ claude-ps
waiting	35	work	0	work-f8	3318865	b08aacbc-…	1755000000	/home/you/Projects/work
idle	5238	notes	1	notes-e1	3132891	f8f9b7ea-…	1754913000	/home/you/notes
busy	13	-	-	scratch-2c	3129839	52b7681e-…	1755004000	/home/you/scratch
```

The third agent is not in zellij, so both join columns are `-`.

### `age` and `started_at` are not the same question

`age` is a duration in the current status; `started_at` is an absolute time the session began.
A consumer needs both, because **a session that has just launched and one that has just
finished a turn are both `idle` with a small `age`** — and a status bar that cannot tell them
apart nags you about every tab you open. Only the launch time separates them.

## Four things it does deliberately

### The status is passed through untouched

The status vocabulary is **open** and moves with Claude Code's version. A
release that emitted `busy`, `idle` and `waiting` was followed by one that also
emits `shell`, and there is no reason that is the last word.

So `claude-ps` never compares the status against a set it knows. Whatever
Claude wrote is what you get. **A consumer should not match on it either** — a
lookup table that renders only the statuses it recognises silently drops live
agents the day Claude invents one.

### Liveness is exact, not a guess

A session file outlives the agent that wrote it. Checking "is this pid alive"
is not enough, because pids are recycled: a stale file whose pid now belongs to
an unrelated process would hand you that process's zellij pane, and a consumer
would send you to a pane that has nothing to do with Claude.

So the check is both halves. The pid must be alive **and** have started when
the file says it did — field 22 of `/proc/<pid>/stat` against the file's
`procStart`. A recycled pid cannot pass.

### The order is for diffing, not for reading

Rows are sorted by session, then pane, then pid, so two runs a second apart
diff cleanly. That is the only reason the order exists.

Deciding what a human should see first is the consumer's job, and consumers
disagree — a picker wants the agent that is waiting on you at the top, a status
bar may want a fixed position per session so it does not jump.

### A missing timestamp is age zero

If none of `statusUpdatedAt`, `updatedAt` or `startedAt` is present, the age is
`0`.

Reading a missing field as epoch-millisecond `0` is the obvious shortcut and it
renders every row as roughly fifty-seven years old, which reads as data rather
than as breakage. If Claude renames those fields, a column of `0s` is visibly
wrong.

## Compatibility

⚠️ **`started_at` was added in a way that breaks the column contract, deliberately.** It sits
**before** `cwd`, because `cwd` has to stay last — it is the only field that may contain
whitespace, which is what lets a consumer take it as the whole remainder of the line.

So a consumer written against the previous eight columns reads `started_at` where it expects
`cwd`. Update consumers and this tool together.

## Consumers

- [zj-picker](https://github.com/lorenzolfm/zj-picker) — its agent screen lists
  every running agent and `Enter` attaches to the session **and** focuses the
  pane. It cannot do this join itself: a zellij plugin's wasi sandbox preopens
  only `/host`, `/data`, `/cache` and `/tmp`, so neither `~/.claude/sessions`
  nor `/proc` is readable from inside it.

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

⚠️ If you use `zj-picker`, the zellij **server's** `PATH` is not your shell's.
The plugin looks this tool up by name there, exactly as it looks up `zoxide`, so
wherever you install it has to be on the `PATH` the server was started with — a
server that predates the install will not see it.

Where the server's `PATH` genuinely lacks it, the plugin's `agents_command`
configuration key names the executable to run instead. That is the supported
escape hatch: an install path compiled into the plugin is what once made it
unusable by anyone but its author.

## Linux only

The join reads `/proc/<pid>/environ` and `/proc/<pid>/stat`. There is no procfs
on darwin, where this would compile and then report no agents at all — so the
flake does not offer a darwin build rather than offering one that is quietly
useless.

## License

MIT
