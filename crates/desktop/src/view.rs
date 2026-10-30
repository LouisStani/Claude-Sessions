//! What the page shows: the session list, built from each refresh.

use claude_sessions_core::accounts::SessionInfo;
use claude_sessions_core::refresher::Update;
use serde::Serialize;

/// Only the first nine sessions get a number, like Alt+1..9 in the terminal.
const NUMBERED: usize = 9;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SessionRow {
    pub number: Option<usize>,
    pub title: String,
    pub account: String,
    pub state: &'static str,
    pub isolated: bool,
    pub unread: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct ListView {
    /// False until the first refresh, so the page can say "loading…" instead of "no sessions".
    pub loaded: bool,
    pub sessions: Vec<SessionRow>,
    pub errors: Vec<String>,
}

fn row(index: usize, session: &SessionInfo, unread: usize) -> SessionRow {
    SessionRow {
        number: (index < NUMBERED).then_some(index + 1),
        title: session.title(),
        account: session.account.clone(),
        state: session.state.label(),
        isolated: session.isolated,
        unread,
    }
}

/// A failed refresh keeps the previous list on screen and says why it is stale.
pub fn apply(previous: &ListView, update: Update) -> ListView {
    match update {
        Ok((snapshot, unread)) => ListView {
            loaded: true,
            sessions: snapshot
                .sessions
                .iter()
                .enumerate()
                .map(|(index, session)| row(index, session, unread.get(&session.session_id).copied().unwrap_or(0)))
                .collect(),
            errors: snapshot.errors,
        },
        Err(error) => ListView {
            loaded: previous.loaded,
            sessions: previous.sessions.clone(),
            errors: vec![format!("session list unavailable: {error}")],
        },
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use claude_sessions_core::accounts::{SessionState, Snapshot};

    use super::*;

    fn session(account: &str, name: &str, isolated: bool) -> SessionInfo {
        SessionInfo {
            account: account.into(),
            short_id: Some("abc".into()),
            session_id: format!("{account}-{name}"),
            name: name.into(),
            cwd: "/home/u/Dev".into(),
            state: SessionState::Waiting,
            isolated,
        }
    }

    fn snapshot(sessions: Vec<SessionInfo>) -> Snapshot {
        Snapshot { sessions, ..Snapshot::default() }
    }

    #[test]
    fn rows_keep_the_snapshot_order_and_only_nine_get_a_number() {
        let sessions = (0..10).map(|i| session("louis", &format!("s{i}"), false)).collect();
        let view = apply(&ListView::default(), Ok((snapshot(sessions), HashMap::new())));
        assert!(view.loaded);
        assert_eq!(view.sessions[0].title, "s0");
        assert_eq!(view.sessions[0].number, Some(1));
        assert_eq!(view.sessions[8].number, Some(9));
        assert_eq!(view.sessions[9].number, None);
        assert_eq!(view.sessions[9].title, "s9");
    }

    #[test]
    fn unread_counts_are_matched_by_session_id() {
        let sessions = vec![session("louis", "a", false), session("lystriq", "b", false)];
        let unread = HashMap::from([("lystriq-b".to_string(), 3)]);
        let view = apply(&ListView::default(), Ok((snapshot(sessions), unread)));
        assert_eq!(view.sessions[0].unread, 0);
        assert_eq!(view.sessions[1].unread, 3);
    }

    #[test]
    fn rows_carry_account_state_and_isolation() {
        let sessions = vec![session("pro", "x", true)];
        let view = apply(&ListView::default(), Ok((snapshot(sessions), HashMap::new())));
        assert_eq!(view.sessions[0].account, "pro");
        assert_eq!(view.sessions[0].state, "waiting");
        assert!(view.sessions[0].isolated);
    }

    #[test]
    fn snapshot_errors_are_shown() {
        let mut broken = snapshot(Vec::new());
        broken.errors.push("louis: agents failed".into());
        let view = apply(&ListView::default(), Ok((broken, HashMap::new())));
        assert_eq!(view.errors, ["louis: agents failed"]);
    }

    #[test]
    fn a_failed_refresh_keeps_the_list_and_says_why() {
        let sessions = vec![session("louis", "a", false)];
        let previous = apply(&ListView::default(), Ok((snapshot(sessions), HashMap::new())));
        let view = apply(&previous, Err("HOME is not set".into()));
        assert_eq!(view.sessions, previous.sessions);
        assert!(view.loaded);
        assert_eq!(view.errors, ["session list unavailable: HOME is not set"]);
    }

}
