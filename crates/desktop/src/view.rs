//! What the page shows: the session list, built from each refresh.

use claude_sessions_core::accounts::{SessionInfo, SessionState};
use claude_sessions_core::refresher::Update;
use claude_sessions_core::search::matches;
use serde::Serialize;

use crate::layout::SessionKey;

/// Only the first nine sessions get a number, like Alt+1..9 in the terminal.
const NUMBERED: usize = 9;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SessionRow {
    pub number: Option<usize>,
    pub title: String,
    pub account: String,
    /// Only background sessions have one, and only they can be opened here.
    pub short_id: Option<String>,
    pub state: &'static str,
    pub isolated: bool,
    pub unread: usize,
    /// Claude finished a turn here and it has not been looked at since.
    pub finished: bool,
    /// Kept for Ctrl+P, which searches the same fields as the terminal picker.
    #[serde(skip)]
    pub info: SessionInfo,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct ListView {
    /// False until the first refresh, so the page can say "loading…" instead of "no sessions".
    pub loaded: bool,
    /// Offered by the Ctrl+N form.
    pub accounts: Vec<String>,
    pub sessions: Vec<SessionRow>,
    pub errors: Vec<String>,
}

fn shows(key: &SessionKey, session: &SessionInfo) -> bool {
    session.account == key.account && session.short_id.as_deref() == Some(key.short_id.as_str())
}

/// Waiting now after working at the last refresh, or already marked; never the session
/// being looked at.
fn finished(previous: &ListView, session: &SessionInfo, watched: Option<&SessionKey>) -> bool {
    if session.state != SessionState::Waiting || watched.is_some_and(|key| shows(key, session)) {
        return false;
    }
    previous
        .sessions
        .iter()
        .find(|row| row.info.session_id == session.session_id)
        .is_some_and(|row| row.finished || row.info.state == SessionState::Working)
}

fn row(index: usize, session: &SessionInfo, unread: usize, finished: bool) -> SessionRow {
    SessionRow {
        number: (index < NUMBERED).then_some(index + 1),
        title: session.title(),
        account: session.account.clone(),
        short_id: session.short_id.clone(),
        state: session.state.label(),
        isolated: session.isolated,
        unread,
        finished,
        info: session.clone(),
    }
}

/// A failed refresh keeps the previous list on screen and says why it is stale.
/// `watched` is the session in the active pane of the visible tab.
pub fn apply(previous: &ListView, update: Update, watched: Option<&SessionKey>) -> ListView {
    match update {
        Ok((snapshot, unread)) => ListView {
            loaded: true,
            accounts: snapshot.accounts,
            sessions: snapshot
                .sessions
                .iter()
                .enumerate()
                .map(|(index, session)| {
                    let unread = unread.get(&session.session_id).copied().unwrap_or(0);
                    row(index, session, unread, finished(previous, session, watched))
                })
                .collect(),
            errors: snapshot.errors,
        },
        Err(error) => ListView {
            loaded: previous.loaded,
            accounts: previous.accounts.clone(),
            sessions: previous.sessions.clone(),
            errors: vec![format!("session list unavailable: {error}")],
        },
    }
}

/// `key` is being looked at: its mark goes. Says whether anything changed.
pub fn seen(view: &mut ListView, key: &SessionKey) -> bool {
    let mut changed = false;
    for row in view.sessions.iter_mut().filter(|row| row.finished && shows(key, &row.info)) {
        row.finished = false;
        changed = true;
    }
    changed
}

/// The rows Ctrl+P offers for `query`, in list order.
pub fn filter(view: &ListView, query: &str) -> Vec<SessionRow> {
    view.sessions.iter().filter(|row| matches(&row.info, query)).cloned().collect()
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

    fn titles(rows: &[SessionRow]) -> Vec<&str> {
        rows.iter().map(|row| row.title.as_str()).collect()
    }

    #[test]
    fn rows_keep_the_snapshot_order_and_only_nine_get_a_number() {
        let sessions = (0..10).map(|i| session("louis", &format!("s{i}"), false)).collect();
        let view = apply(&ListView::default(), Ok((snapshot(sessions), HashMap::new())), None);
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
        let view = apply(&ListView::default(), Ok((snapshot(sessions), unread)), None);
        assert_eq!(view.sessions[0].unread, 0);
        assert_eq!(view.sessions[1].unread, 3);
    }

    #[test]
    fn rows_carry_account_id_state_and_isolation() {
        let sessions = vec![session("pro", "x", true)];
        let view = apply(&ListView::default(), Ok((snapshot(sessions), HashMap::new())), None);
        assert_eq!(view.sessions[0].account, "pro");
        assert_eq!(view.sessions[0].short_id.as_deref(), Some("abc"));
        assert_eq!(view.sessions[0].state, "waiting");
        assert!(view.sessions[0].isolated);
    }

    #[test]
    fn snapshot_errors_are_shown() {
        let mut broken = snapshot(Vec::new());
        broken.errors.push("louis: agents failed".into());
        let view = apply(&ListView::default(), Ok((broken, HashMap::new())), None);
        assert_eq!(view.errors, ["louis: agents failed"]);
    }

    #[test]
    fn a_failed_refresh_keeps_the_list_and_says_why() {
        let sessions = vec![session("louis", "a", false)];
        let previous = apply(&ListView::default(), Ok((snapshot(sessions), HashMap::new())), None);
        let view = apply(&previous, Err("HOME is not set".into()), None);
        assert_eq!(view.sessions, previous.sessions);
        assert!(view.loaded);
        assert_eq!(view.errors, ["session list unavailable: HOME is not set"]);
    }

    #[test]
    fn accounts_come_from_the_snapshot_and_survive_a_failed_refresh() {
        let listed = Snapshot { accounts: vec!["louis".into(), "lystriq".into()], ..Snapshot::default() };
        let view = apply(&ListView::default(), Ok((listed, HashMap::new())), None);
        assert_eq!(view.accounts, ["louis", "lystriq"]);
        let failed = apply(&view, Err("HOME is not set".into()), None);
        assert_eq!(failed.accounts, ["louis", "lystriq"]);
    }

    #[test]
    fn filter_keeps_matching_rows_in_list_order() {
        let mut sessions = vec![session("louis", "boss-fight", false), session("lystriq", "api", false)];
        sessions.push(session("louis", "api", false));
        let view = apply(&ListView::default(), Ok((snapshot(sessions), HashMap::new())), None);
        assert_eq!(titles(&filter(&view, "")), ["boss-fight", "api", "api"]);
        assert_eq!(titles(&filter(&view, "API")), ["api", "api"]);
        let both = filter(&view, "api louis");
        assert_eq!(both.len(), 1);
        assert_eq!(both[0].account, "louis");
        assert!(filter(&view, "zzz").is_empty());
    }

    fn in_state(account: &str, name: &str, state: SessionState) -> SessionInfo {
        SessionInfo { state, short_id: Some(name.into()), ..session(account, name, false) }
    }

    fn refresh(previous: &ListView, sessions: Vec<SessionInfo>, watched: Option<&SessionKey>) -> ListView {
        apply(previous, Ok((snapshot(sessions), HashMap::new())), watched)
    }

    fn marks(view: &ListView) -> Vec<bool> {
        view.sessions.iter().map(|row| row.finished).collect()
    }

    fn key(account: &str, short_id: &str) -> SessionKey {
        SessionKey { account: account.into(), short_id: short_id.into() }
    }

    #[test]
    fn a_session_that_stops_working_is_marked_until_it_works_again() {
        let working = refresh(&ListView::default(), vec![in_state("louis", "a", SessionState::Working)], None);
        assert_eq!(marks(&working), [false]);
        let done = refresh(&working, vec![in_state("louis", "a", SessionState::Waiting)], None);
        assert_eq!(marks(&done), [true]);
        let still = refresh(&done, vec![in_state("louis", "a", SessionState::Waiting)], None);
        assert_eq!(marks(&still), [true]);
        let again = refresh(&still, vec![in_state("louis", "a", SessionState::Working)], None);
        assert_eq!(marks(&again), [false]);
    }

    #[test]
    fn resumed_sessions_and_the_first_list_are_not_marked() {
        let first = refresh(&ListView::default(), vec![in_state("louis", "a", SessionState::Waiting)], None);
        assert_eq!(marks(&first), [false]);
        let paused = refresh(&first, vec![in_state("louis", "a", SessionState::Paused)], None);
        let resumed = refresh(&paused, vec![in_state("louis", "a", SessionState::Waiting)], None);
        assert_eq!(marks(&resumed), [false]);
    }

    #[test]
    fn the_watched_session_is_never_marked() {
        let working = vec![in_state("louis", "a", SessionState::Working), in_state("louis", "b", SessionState::Working)];
        let working = refresh(&ListView::default(), working, None);
        let waiting = vec![in_state("louis", "a", SessionState::Waiting), in_state("louis", "b", SessionState::Waiting)];
        let done = refresh(&working, waiting, Some(&key("louis", "a")));
        assert_eq!(marks(&done), [false, true]);
    }

    #[test]
    fn looking_at_a_session_removes_its_mark() {
        let working = refresh(&ListView::default(), vec![in_state("louis", "a", SessionState::Working)], None);
        let mut done = refresh(&working, vec![in_state("louis", "a", SessionState::Waiting)], None);
        assert!(!seen(&mut done, &key("lystriq", "a")), "another account");
        assert_eq!(marks(&done), [true]);
        assert!(seen(&mut done, &key("louis", "a")));
        assert_eq!(marks(&done), [false]);
        assert!(!seen(&mut done, &key("louis", "a")), "nothing left to remove");
    }

    #[test]
    fn a_failed_refresh_keeps_the_marks() {
        let working = refresh(&ListView::default(), vec![in_state("louis", "a", SessionState::Working)], None);
        let done = refresh(&working, vec![in_state("louis", "a", SessionState::Waiting)], None);
        let failed = apply(&done, Err("HOME is not set".into()), None);
        assert_eq!(marks(&failed), [true]);
    }
}
