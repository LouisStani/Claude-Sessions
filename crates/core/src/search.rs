use crate::accounts::SessionInfo;

/// Every word of `query` must appear, case-insensitively, in the session's title, account or directory.
pub fn matches(session: &SessionInfo, query: &str) -> bool {
    let haystack = format!("{} {} {}", session.title(), session.account, session.cwd).to_lowercase();
    query.to_lowercase().split_whitespace().all(|word| haystack.contains(word))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::accounts::SessionState;

    fn session(account: &str, name: &str, cwd: &str) -> SessionInfo {
        SessionInfo {
            account: account.into(),
            short_id: Some("abc".into()),
            session_id: format!("{account}-{name}"),
            name: name.into(),
            cwd: cwd.into(),
            state: SessionState::Waiting,
            isolated: false,
        }
    }

    #[test]
    fn every_word_must_match_somewhere() {
        assert!(matches(&session("louis", "boss-fight", "/home/u/Roblox/frieren"), "ROB louis"));
        assert!(matches(&session("louis", "", "/home/u/Merkio"), "merkio"));
        assert!(!matches(&session("lystriq", "api", "/home/u/Pro/api"), "api louis"));
    }
}
