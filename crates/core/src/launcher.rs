//! Starts a background Claude Code session for an account, in a directory.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use claude_accounts::{Paths, launch, mcp};

#[derive(Debug, PartialEq, Eq)]
pub struct Request {
    pub account: String,
    pub directory: PathBuf,
    pub name: String,
}

pub fn expand_home(path: &str, home: &Path) -> PathBuf {
    match path.strip_prefix('~') {
        Some(rest) if rest.is_empty() || rest.starts_with('/') => home.join(rest.trim_start_matches('/')),
        _ => PathBuf::from(path),
    }
}

/// Reads the short id from `backgrounded · 378ce649 · name (…)`.
pub fn parse_backgrounded_id(output: &str) -> Option<String> {
    output.lines().find_map(|line| {
        let rest = line.trim().strip_prefix("backgrounded · ")?;
        let id = rest.split_whitespace().next()?;
        id.chars().all(|c| c.is_ascii_alphanumeric()).then(|| id.to_string())
    })
}

pub struct Launched {
    pub short_id: String,
    pub warnings: Vec<String>,
}

fn library_error(error: claude_accounts::Error) -> String {
    error.to_string()
}

/// Starts a background session the way `claude-accounts run <account> --bg` does: shared
/// MCP servers (none for an isolated account), shared settings, billing variables removed.
pub fn launch(request: &Request) -> Result<Launched, String> {
    let paths = Paths::from_env().map_err(library_error)?;
    launch::require_account(&paths, &request.account).map_err(library_error)?;
    launch::require_claude().map_err(library_error)?;
    let shared = mcp::prepare(&paths, &request.account).map_err(library_error)?;
    let mut args: Vec<OsString> = vec!["--bg".into()];
    if !request.name.is_empty() {
        args.push("-n".into());
        args.push(request.name.clone().into());
    }
    let prepared =
        launch::prepare(&paths, &request.account, shared.file.as_deref(), &args).map_err(library_error)?;
    let mut command = prepared.command;
    let output = command
        .current_dir(&request.directory)
        .stdin(Stdio::null())
        .output()
        .map_err(|error| format!("claude: {error}"))?;
    let text = format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    if !output.status.success() {
        return Err(format!("cannot start the session: {}", text.trim()));
    }
    let short_id = parse_backgrounded_id(&text).ok_or_else(|| format!("no session id in: {}", text.trim()))?;
    let warnings = shared.warnings.into_iter().chain(prepared.warnings).collect();
    Ok(Launched { short_id, warnings })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backgrounded_id_is_read_from_the_first_line() {
        let output = "backgrounded · 378ce649 · proto-bg-test (idle — send a prompt to start)\n  claude agents  list";
        assert_eq!(parse_backgrounded_id(output), Some("378ce649".into()));
        assert_eq!(parse_backgrounded_id("error: nope"), None);
        assert_eq!(parse_backgrounded_id("backgrounded · ../x · y"), None);
    }

    #[test]
    fn home_is_expanded_only_at_the_start() {
        let home = Path::new("/home/u");
        assert_eq!(expand_home("~/Dev", home), PathBuf::from("/home/u/Dev"));
        assert_eq!(expand_home("~", home), PathBuf::from("/home/u"));
        assert_eq!(expand_home("~other/x", home), PathBuf::from("~other/x"));
        assert_eq!(expand_home("/tmp", home), PathBuf::from("/tmp"));
    }
}
