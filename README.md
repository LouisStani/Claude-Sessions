# claude-sessions

Several Claude Code sessions, across several accounts, side by side in one terminal. Sessions can leave each other short messages.

claude-sessions is the companion of [claude-accounts](https://github.com/LouisStani/Claude-Accounts). claude-accounts runs each account with its own login and a shared setup. claude-sessions shows the sessions of all those accounts in one place.

```text
┌ sessions ─────────────┐┌ roblox · louis ─────────────┐┌ api · lystriq ──────────────┐
│1 roblox               ││                             ││                             │
│  louis · working      ││  Claude Code                ││  Claude Code                │
│2 api 1 msg            ││                             ││                             │
│  lystriq · waiting    ││ ❯                           ││ ❯                           │
└ Ctrl+P · Ctrl+N ──────┘└─────────────────────────────┘└─────────────────────────────┘
```

## What it does

- Lists the Claude Code sessions of every account managed by claude-accounts, with their state.
- Shows two sessions side by side. Each pane is a real Claude Code session: everything you type goes to it.
- Starts new background sessions on the account and in the directory you choose.
- Lets sessions send each other messages through a small MCP server, across accounts.

Closing claude-sessions closes nothing: the sessions are Claude Code background sessions and keep running. Open claude-sessions again and they are still there.

## Requirements

- [claude-accounts](https://github.com/LouisStani/Claude-Accounts) 1.2.0 or later, on your `PATH`
- Claude Code with background sessions (`claude --bg`, `claude attach`, `claude agents`). Tested with 2.1.312.
- Rust, to build
- Linux. Other systems are untested.

## Installation

```sh
git clone https://github.com/LouisStani/Claude-Sessions.git claude-sessions
cargo install --locked --path claude-sessions
```

The binary goes to `~/.cargo/bin`. Make sure it is on your `PATH`.

To let sessions send messages, add the MCP server once, in any account, with the user scope:

```sh
claude-accounts run <account> mcp add -s user claude-sessions -- claude-sessions mcp
```

claude-accounts shares it with your other accounts. Sessions started after this have it; sessions already running do not.

## Usage

```sh
claude-sessions
```

| Key | Effect |
| --- | --- |
| `Alt+1` … `Alt+9` | Shows session 1 to 9 of the list in the active pane |
| `Ctrl+P` | Finds a session by title, account or directory. Its last entry redraws the screen. |
| `Ctrl+N` | Starts a new background session: account, directory, optional name |
| `Ctrl+H` / `Ctrl+L` | Makes the left or right pane active |
| `Ctrl+Q` | Quits. The sessions keep running. |

Every other key goes to the session in the active pane.

claude-sessions keeps `Ctrl+H`, `Ctrl+L`, `Ctrl+N`, `Ctrl+P` and `Ctrl+Q` for itself. Claude Code uses `Ctrl+N` and `Ctrl+P` like the arrow keys, which still work. It uses `Ctrl+L` to redraw its screen: use the redraw entry of `Ctrl+P` instead.

### Session states

| State | Meaning |
| --- | --- |
| `working` | Claude is answering |
| `waiting` | Claude waits for you |
| `paused` | Background session with no process. Opening it wakes it up. |
| `elsewhere` | Interactive session running in another terminal. It can receive messages but cannot be shown here. |

`isolated` after the state marks a session of an isolated account.

`2 msg` after a title means the session has two unread messages.

## Messages between sessions

The MCP server gives every session three tools:

| Tool | Effect |
| --- | --- |
| `list_sessions` | Lists the sessions of all accounts: title, account, state, id, directory |
| `send_message` | Leaves a message for another session, by title or id |
| `read_messages` | Returns the unread messages of this session, once |

A session reads its messages only when it calls `read_messages`, for example when you ask it to. Nothing is typed into a session for you.

Messages are information, not orders. The tools tell Claude to treat them that way and to ask you before acting on a request they contain. In testing, a session that received "please create this file" did not create it and asked the user first.

Isolated accounts (`claude-accounts isolate <account>`, for a work account for example) stay out of messages: their sessions never appear in `list_sessions`, cannot receive messages, and the MCP server refuses to start in them. They can still be shown in a pane.

Messages are stored as one JSON file each in `~/.local/share/claude-sessions/mailbox/<session id>/` (or under `$XDG_DATA_HOME`), in directories readable only by you. A message that has been read moves to `read/`.

## How it works

- The session list comes from `claude-accounts run <account> agents --json`, for every account.
- A pane runs `claude-accounts run <account> attach <id>` in its own pseudo-terminal. [alacritty_terminal](https://crates.io/crates/alacritty_terminal) keeps its screen and [ratatui](https://ratatui.rs) draws it.
- `Ctrl+N` runs `claude-accounts run <account> --bg`, then opens the new session.
- The list refreshes when Claude Code writes to an account's `sessions/`, `daemon/` or `jobs/` directory, or when a message arrives, and every 30 seconds in case a change was missed. Idle, claude-sessions uses about 5% of one CPU core.

claude-sessions never reads credentials. All account handling goes through claude-accounts.

## Limits

- Mouse events are not forwarded to Claude Code.
- An interactive session started in another terminal cannot be shown in a pane: only background sessions can be attached.
- Resuming a background session through `claude-accounts run … --bg --resume` starts a copy of it, because claude-accounts always passes its own options. To continue the session itself, run `claude --bg --resume <id>` with no other option and `CLAUDE_CONFIG_DIR` set to the account directory.

## Development

```sh
cargo test
cargo clippy --all-targets
```

The tests never start Claude Code and never touch your accounts.

## License

claude-sessions is licensed under [CC BY-NC-SA 4.0](LICENSE). You can use, change and share it for non-commercial purposes, if you credit the author and share your changes under the same license.

Commercial use, including use inside a company, is not allowed by this license. To ask for it, open an issue.
