//! What the page shows: the session list, built from each refresh.

use claude_sessions_core::accounts::{SessionInfo, SessionState};
use claude_sessions_core::refresher::Update;
use claude_sessions_core::search::matches;
use serde::Serialize;

use crate::layout::SessionKey;

/// Only the first nine sessions of the bar get a number: Alt+1..9.
const NUMBERED: usize = 9;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SessionRow {
    pub number: Option<usize>,
    pub title: String,
    pub account: String,
    pub directory: String,
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
    /// Place in the snapshot, which orders sessions of the same rank.
    #[serde(skip)]
    position: usize,
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

fn row(position: usize, session: &SessionInfo, unread: usize, finished: bool) -> SessionRow {
    SessionRow {
        number: None,
        title: session.title(),
        account: session.account.clone(),
        directory: session.cwd.clone(),
        short_id: session.short_id.clone(),
        state: session.state.label(),
        isolated: session.isolated,
        unread,
        finished,
        info: session.clone(),
        position,
    }
}

/// Where a session goes in the bar: what needs you first, then what runs, then what sleeps.
/// Sessions running in another terminal cannot be opened here: always last.
fn rank(row: &SessionRow) -> u8 {
    if row.short_id.is_none() {
        return 5;
    }
    if row.unread > 0 {
        return 0;
    }
    if row.finished {
        return 1;
    }
    match row.info.state {
        SessionState::Waiting => 2,
        SessionState::Working => 3,
        SessionState::Paused => 4,
        SessionState::Elsewhere => 5,
    }
}

/// Sorts the rows by `rank`, in snapshot order within a rank, and numbers them.
fn order(sessions: &mut [SessionRow]) {
    sessions.sort_by_key(|row| (rank(row), row.position));
    for (index, row) in sessions.iter_mut().enumerate() {
        row.number = (index < NUMBERED).then_some(index + 1);
    }
}

/// A failed refresh keeps the previous list on screen and says why it is stale.
/// `watched` is the session in the active pane of the visible tab.
pub fn apply(previous: &ListView, update: Update, watched: Option<&SessionKey>) -> ListView {
    match update {
        Ok((snapshot, unread)) => {
            let mut sessions: Vec<SessionRow> = snapshot
                .sessions
                .iter()
                .enumerate()
                .map(|(position, session)| {
                    let unread = unread.get(&session.session_id).copied().unwrap_or(0);
                    row(position, session, unread, finished(previous, session, watched))
                })
                .collect();
            order(&mut sessions);
            ListView { loaded: true, accounts: snapshot.accounts, sessions, errors: snapshot.errors }
        }
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
    if changed {
        order(&mut view.sessions);
    }
    changed
}

/// The rows Ctrl+P offers for `query`, in bar order.
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

    fn unread_of(view: &ListView, title: &str) -> usize {
        view.sessions.iter().find(|row| row.title == title).map(|row| row.unread).unwrap()
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
        assert_eq!(unread_of(&view, "a"), 0);
        assert_eq!(unread_of(&view, "b"), 3);
    }

    #[test]
    fn rows_carry_account_directory_id_state_and_isolation() {
        let sessions = vec![session("pro", "x", true)];
        let view = apply(&ListView::default(), Ok((snapshot(sessions), HashMap::new())), None);
        assert_eq!(view.sessions[0].account, "pro");
        assert_eq!(view.sessions[0].directory, "/home/u/Dev");
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
        assert_eq!(titles(&done.sessions), ["b", "a"]);
        assert_eq!(marks(&done), [true, false]);
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

    fn numbers(view: &ListView) -> Vec<Option<usize>> {
        view.sessions.iter().map(|row| row.number).collect()
    }

    fn elsewhere(account: &str, name: &str) -> SessionInfo {
        SessionInfo { short_id: None, ..in_state(account, name, SessionState::Elsewhere) }
    }

    #[test]
    fn the_bar_puts_what_needs_you_first() {
        let before = vec![in_state("louis", "done", SessionState::Working)];
        let previous = refresh(&ListView::default(), before, None);
        let sessions = vec![
            elsewhere("louis", "other-terminal"),
            in_state("louis", "asleep", SessionState::Paused),
            in_state("louis", "busy", SessionState::Working),
            in_state("louis", "waits", SessionState::Waiting),
            in_state("louis", "done", SessionState::Waiting),
            in_state("louis", "mail", SessionState::Working),
        ];
        let unread = HashMap::from([("louis-mail".to_string(), 1)]);
        let view = apply(&previous, Ok((snapshot(sessions), unread)), None);
        assert_eq!(titles(&view.sessions), ["mail", "done", "waits", "busy", "asleep", "other-terminal"]);
        assert_eq!(numbers(&view), [Some(1), Some(2), Some(3), Some(4), Some(5), Some(6)]);
    }

    #[test]
    fn sessions_of_another_terminal_stay_last_even_with_messages() {
        let sessions = vec![elsewhere("louis", "other-terminal"), in_state("louis", "asleep", SessionState::Paused)];
        let unread = HashMap::from([("louis-other-terminal".to_string(), 2)]);
        let view = apply(&ListView::default(), Ok((snapshot(sessions), unread)), None);
        assert_eq!(titles(&view.sessions), ["asleep", "other-terminal"]);
    }

    #[test]
    fn sessions_of_the_same_rank_keep_the_snapshot_order() {
        let states = [SessionState::Paused, SessionState::Waiting];
        let sessions = (0..40).map(|i| in_state("louis", &format!("s{i:02}"), states[i % 2])).collect();
        let view = refresh(&ListView::default(), sessions, None);
        let waiting: Vec<String> = (0..40).filter(|i| i % 2 == 1).map(|i| format!("s{i:02}")).collect();
        let paused: Vec<String> = (0..40).filter(|i| i % 2 == 0).map(|i| format!("s{i:02}")).collect();
        assert_eq!(titles(&view.sessions[..20]), waiting);
        assert_eq!(titles(&view.sessions[20..]), paused);
    }

    #[test]
    fn a_session_looked_at_moves_down_with_its_mark() {
        let working = vec![in_state("louis", "a", SessionState::Waiting), in_state("louis", "b", SessionState::Working)];
        let working = refresh(&ListView::default(), working, None);
        let waiting = vec![in_state("louis", "a", SessionState::Waiting), in_state("louis", "b", SessionState::Waiting)];
        let mut done = refresh(&working, waiting, None);
        assert_eq!(titles(&done.sessions), ["b", "a"]);
        assert!(seen(&mut done, &key("louis", "b")));
        assert_eq!(titles(&done.sessions), ["a", "b"]);
        assert_eq!(numbers(&done), [Some(1), Some(2)]);
    }

    #[test]
    fn a_failed_refresh_keeps_the_marks() {
        let working = refresh(&ListView::default(), vec![in_state("louis", "a", SessionState::Working)], None);
        let done = refresh(&working, vec![in_state("louis", "a", SessionState::Waiting)], None);
        let failed = apply(&done, Err("HOME is not set".into()), None);
        assert_eq!(marks(&failed), [true]);
    }
}
