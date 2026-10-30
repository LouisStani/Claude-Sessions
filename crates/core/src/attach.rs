//! The commands that show a background session in a terminal or stop it, the same for every
//! interface.

use std::process::Command;

use crate::accounts::LAUNCHER;

pub struct AttachCommand {
    pub program: String,
    pub args: Vec<String>,
    /// Claude Code draws in colour only when the terminal says it can.
    pub env: Vec<(String, String)>,
}

/// `claude-accounts run <account> attach <short_id>`: claude-accounts sets the account up,
/// then Claude Code attaches. Ending this process only detaches; the session keeps running.
pub fn attach_command(account: &str, short_id: &str) -> AttachCommand {
    AttachCommand {
        program: LAUNCHER.into(),
        args: ["run", account, "attach", short_id].map(String::from).to_vec(),
        env: vec![("TERM".into(), "xterm-256color".into()), ("COLORTERM".into(), "truecolor".into())],
    }
}

/// `claude-accounts run <account> stop <short_id>`: the session stops and keeps its
/// conversation; `attach` starts it again.
pub fn stop_command(account: &str, short_id: &str) -> Command {
    let mut command = Command::new(LAUNCHER);
    command.args(["run", account, "stop", short_id]);
    command
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attaches_through_claude_accounts_with_colours() {
        let command = attach_command("lystriq", "378ce649");
        assert_eq!(command.program, "claude-accounts");
        assert_eq!(command.args, ["run", "lystriq", "attach", "378ce649"]);
        assert!(command.env.contains(&("TERM".into(), "xterm-256color".into())));
        assert!(command.env.contains(&("COLORTERM".into(), "truecolor".into())));
    }

    #[test]
    fn stops_through_claude_accounts() {
        let command = stop_command("lystriq", "378ce649");
        assert_eq!(command.get_program(), "claude-accounts");
        assert_eq!(command.get_args().collect::<Vec<_>>(), ["run", "lystriq", "stop", "378ce649"]);
    }
}
