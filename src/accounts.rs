use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;

use claude_accounts::{Paths, launch, registry};
use serde::Deserialize;

const LAUNCHER: &str = "claude-accounts";
pub const MIN_LAUNCHER_VERSION: (u32, u32, u32) = (1, 2, 0);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionState {
    Working,
    Waiting,
    Paused,
    Elsewhere,
}

impl SessionState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Working => "working",
            Self::Waiting => "waiting",
            Self::Paused => "paused",
            Self::Elsewhere => "elsewhere",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionInfo {
    pub account: String,
    /// Short id from `claude --bg`; only background sessions have one.
    pub short_id: Option<String>,
    pub session_id: String,
    pub name: String,
    pub cwd: String,
    pub state: SessionState,
    /// Account isolated by `claude-accounts isolate`: kept out of messages.
    pub isolated: bool,
}

impl SessionInfo {
    pub fn can_attach(&self) -> bool {
        self.short_id.is_some()
    }

    /// `name` if set, else the last directory of `cwd`.
    pub fn title(&self) -> String {
        if !self.name.is_empty() {
            return self.name.clone();
        }
        Path::new(&self.cwd)
            .file_name()
            .map_or_else(|| self.cwd.clone(), |dir| dir.to_string_lossy().into_owned())
    }
}

#[derive(Deserialize)]
struct RawSession {
    id: Option<String>,
    #[serde(rename = "sessionId", default)]
    session_id: String,
    name: Option<String>,
    kind: Option<String>,
    status: Option<String>,
    pid: Option<u32>,
    cwd: Option<String>,
}

fn state_of(raw: &RawSession) -> SessionState {
    if raw.kind.as_deref() != Some("background") {
        return SessionState::Elsewhere;
    }
    if raw.pid.is_none() {
        return SessionState::Paused;
    }
    match raw.status.as_deref() {
        Some("busy") => SessionState::Working,
        _ => SessionState::Waiting,
    }
}

/// Parses the output of `claude agents --json` for one account.
pub fn parse_agents(account: &str, json: &str) -> serde_json::Result<Vec<SessionInfo>> {
    let raw: Vec<RawSession> = serde_json::from_str(json)?;
    Ok(raw
        .into_iter()
        .map(|raw| SessionInfo {
            account: account.to_string(),
            state: state_of(&raw),
            short_id: raw.id,
            session_id: raw.session_id,
            name: raw.name.unwrap_or_default(),
            cwd: raw.cwd.unwrap_or_default(),
            isolated: false,
        })
        .collect())
}

/// Parses `claude-accounts 1.2.3` into (1, 2, 3).
pub fn parse_version(output: &str) -> Option<(u32, u32, u32)> {
    let version = output.trim().strip_prefix("claude-accounts ")?;
    let mut parts = version.split('.').map(str::parse::<u32>);
    match (parts.next(), parts.next(), parts.next(), parts.next()) {
        (Some(Ok(major)), Some(Ok(minor)), Some(Ok(patch)), None) => Some((major, minor, patch)),
        _ => None,
    }
}

fn launcher_output(args: &[&str]) -> io::Result<String> {
    let output = Command::new(LAUNCHER).args(args).stdin(Stdio::null()).output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(io::Error::other(format!("{LAUNCHER} {}: {}", args.join(" "), stderr.trim())));
    }
    String::from_utf8(output.stdout).map_err(io::Error::other)
}

pub fn check_launcher_version() -> io::Result<()> {
    let output = launcher_output(&["--version"])?;
    let version = parse_version(&output)
        .ok_or_else(|| io::Error::other(format!("unexpected version output: {}", output.trim())))?;
    if version < MIN_LAUNCHER_VERSION {
        let (major, minor, patch) = MIN_LAUNCHER_VERSION;
        return Err(io::Error::other(format!(
            "{LAUNCHER} {major}.{minor}.{patch} or later is required, found {}",
            output.trim()
        )));
    }
    Ok(())
}

fn library_error(error: claude_accounts::Error) -> io::Error {
    io::Error::other(error.to_string())
}

pub fn paths() -> io::Result<Paths> {
    Paths::from_env().map_err(library_error)
}

pub fn account_dirs(accounts: &[String]) -> io::Result<Vec<PathBuf>> {
    let paths = paths()?;
    Ok(accounts.iter().map(|account| paths.account_dir(account)).collect())
}

pub fn isolated_accounts() -> io::Result<Vec<String>> {
    registry::list_isolated(&paths()?).map_err(library_error)
}

pub fn mark_isolated(sessions: &mut [SessionInfo], isolated: &[String]) {
    for session in sessions {
        session.isolated = isolated.contains(&session.account);
    }
}

/// Sessions that may take part in messages: never those of an isolated account.
pub fn messaging_sessions(sessions: Vec<SessionInfo>) -> Vec<SessionInfo> {
    sessions.into_iter().filter(|session| !session.isolated).collect()
}

/// `claude agents --json` on one account, with the environment rules of
/// `claude-accounts run`. The shared configuration is not linked again here: relinking
/// removes then recreates the links, and this runs at every refresh.
fn account_sessions(paths: &Paths, account: &str) -> io::Result<Vec<SessionInfo>> {
    let dir = launch::require_account(paths, account).map_err(library_error)?;
    let claude = launch::require_claude().map_err(library_error)?;
    let mut command = Command::new(claude);
    for variable in launch::STRIPPED_ENV {
        command.env_remove(variable);
    }
    let output = command.env("CLAUDE_CONFIG_DIR", &dir).args(["agents", "--json"]).stdin(Stdio::null()).output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(io::Error::other(format!("claude agents --json: {}", stderr.trim())));
    }
    parse_agents(account, &String::from_utf8_lossy(&output.stdout)).map_err(io::Error::other)
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Snapshot {
    pub accounts: Vec<String>,
    pub isolated: Vec<String>,
    /// Sorted by account, then title, so list positions (Alt+1..9) stay stable.
    pub sessions: Vec<SessionInfo>,
    pub errors: Vec<String>,
}

pub fn sort_sessions(sessions: &mut [SessionInfo]) {
    sessions.sort_by_cached_key(|session| {
        (session.account.clone(), session.title().to_lowercase(), session.session_id.clone())
    });
}

/// Sessions of every account, accounts queried in parallel. An account that fails is
/// reported in `errors` and skipped, so one broken account never hides the others.
pub fn snapshot() -> io::Result<Snapshot> {
    let paths = paths()?;
    let accounts = registry::list_accounts(&paths).map_err(library_error)?;
    let isolated = registry::list_isolated(&paths).map_err(library_error)?;
    let handles: Vec<_> = accounts
        .iter()
        .cloned()
        .map(|account| {
            let paths = paths.clone();
            thread::spawn(move || (account.clone(), account_sessions(&paths, &account)))
        })
        .collect();
    let mut sessions = Vec::new();
    let mut errors = Vec::new();
    for handle in handles {
        match handle.join() {
            Ok((_, Ok(found))) => sessions.extend(found),
            Ok((account, Err(error))) => errors.push(format!("{account}: {error}")),
            Err(_) => errors.push("account query thread panicked".to_string()),
        }
    }
    mark_isolated(&mut sessions, &isolated);
    sort_sessions(&mut sessions);
    Ok(Snapshot { accounts, isolated, sessions, errors })
}

#[cfg(test)]
mod tests {
    use super::*;

    const AGENTS: &str = r#"[
        {"id":"a40b6e33","name":"boss-fight","kind":"background","status":"busy","pid":170869,
         "cwd":"/home/u/Roblox/frieren-rpg","sessionId":"a40b6e33-cde6"},
        {"id":"bc582e37","name":"Router","kind":"background","cwd":"/home/u","sessionId":"bc582e37-1715"},
        {"id":"378ce649","name":"","kind":"background","status":"idle","pid":190760,
         "cwd":"/home/u/proto","sessionId":"378ce649-5ad1"},
        {"name":"frieren-rpg-8d","kind":"interactive","status":"busy","pid":188510,
         "cwd":"/home/u/frieren","sessionId":"25305016-fd5d"}
    ]"#;

    #[test]
    fn agents_json_maps_to_states() {
        let sessions = parse_agents("louis", AGENTS).unwrap();
        let states: Vec<_> = sessions.iter().map(|s| s.state).collect();
        assert_eq!(
            states,
            [SessionState::Working, SessionState::Paused, SessionState::Waiting, SessionState::Elsewhere]
        );
        assert!(sessions.iter().all(|s| s.account == "louis"));
        assert_eq!(sessions[0].short_id.as_deref(), Some("a40b6e33"));
        assert!(!sessions[3].can_attach());
    }

    #[test]
    fn title_falls_back_to_directory() {
        let sessions = parse_agents("lystriq", AGENTS).unwrap();
        assert_eq!(sessions[0].title(), "boss-fight");
        assert_eq!(sessions[2].title(), "proto");
    }

    #[test]
    fn sessions_sort_by_account_then_title() {
        let mut sessions = parse_agents("pro", AGENTS).unwrap();
        sessions.extend(parse_agents("louis", AGENTS).unwrap());
        sort_sessions(&mut sessions);
        let order: Vec<_> = sessions.iter().map(|s| (s.account.as_str(), s.title())).collect();
        assert_eq!(order[0], ("louis", "boss-fight".to_string()));
        assert_eq!(order[3], ("louis", "Router".to_string()));
        assert_eq!(order[4].0, "pro");
    }

    #[test]
    fn isolated_accounts_are_marked_and_kept_out_of_messages() {
        let mut sessions = parse_agents("louis", AGENTS).unwrap();
        sessions.extend(parse_agents("pro", AGENTS).unwrap());
        mark_isolated(&mut sessions, &["pro".to_string()]);
        assert!(sessions.iter().all(|s| s.isolated == (s.account == "pro")));
        let messaging = messaging_sessions(sessions);
        assert_eq!(messaging.len(), 4);
        assert!(messaging.iter().all(|s| s.account == "louis"));
    }

    #[test]
    fn broken_json_is_an_error() {
        assert!(parse_agents("x", "[{").is_err());
    }

    #[test]
    fn version_parsing() {
        assert_eq!(parse_version("claude-accounts 1.1.0\n"), Some((1, 1, 0)));
        assert_eq!(parse_version("claude-accounts 1.0"), None);
        assert_eq!(parse_version("other 1.1.0"), None);
        assert!(parse_version("claude-accounts 1.0.9").unwrap() < MIN_LAUNCHER_VERSION);
        assert!(parse_version("claude-accounts 1.10.0").unwrap() > MIN_LAUNCHER_VERSION);
    }
}
