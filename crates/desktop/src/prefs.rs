//! What the app keeps about a session, by session id, in `sessions.json` (see `store`): the
//! short name given with F2.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// A name of more characters than this is cut in the bar.
const SHORT: usize = 24;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionPrefs {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

impl SessionPrefs {
    fn is_empty(&self) -> bool {
        self.label.is_none()
    }
}

pub type Prefs = BTreeMap<String, SessionPrefs>;

/// Gives `session_id` the short name `label`; an empty one goes back to Claude Code's name.
pub fn set_label(prefs: &mut Prefs, session_id: &str, label: &str) {
    let label = label.trim();
    let entry = prefs.entry(session_id.to_string()).or_default();
    entry.label = (!label.is_empty()).then(|| label.to_string());
    if entry.is_empty() {
        prefs.remove(session_id);
    }
}

/// What a chip shows: the short name given, else Claude Code's name, cut after the last whole
/// word that fits, with "…".
pub fn short_title(label: Option<&str>, name: &str) -> String {
    if let Some(label) = label {
        return label.to_string();
    }
    if name.chars().count() <= SHORT {
        return name.to_string();
    }
    let kept: String = name.chars().take(SHORT - 1).collect();
    let word_ends_here = name.chars().nth(SHORT - 1) == Some(' ');
    let cut = match kept.rfind(' ') {
        Some(space) if space > 0 && !word_ends_here => &kept[..space],
        _ => kept.as_str(),
    };
    format!("{}…", cut.trim_end())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_given_name_comes_first() {
        assert_eq!(short_title(Some("router"), "Router CEO et flux de données Overview"), "router");
    }

    #[test]
    fn a_short_enough_name_is_kept() {
        assert_eq!(short_title(None, "migrations"), "migrations");
        assert_eq!(short_title(None, "abcdefghijklmnopqrstuvwx"), "abcdefghijklmnopqrstuvwx");
    }

    #[test]
    fn a_long_name_is_cut_after_a_whole_word() {
        let short = short_title(None, "Router CEO et flux de données Overview");
        assert_eq!(short, "Router CEO et flux de…");
        assert!(short.chars().count() <= SHORT);
    }

    #[test]
    fn a_word_ending_at_the_limit_is_kept_whole() {
        assert_eq!(short_title(None, "aaaaa bbbbb ccccc ddddd eeee"), "aaaaa bbbbb ccccc ddddd…");
    }

    #[test]
    fn one_long_word_is_cut_where_it_must() {
        assert_eq!(short_title(None, "ajouter-aide-dossiers-champs-desktop"), "ajouter-aide-dossiers-c…");
    }

    #[test]
    fn characters_are_counted_not_bytes() {
        let name = "éééééééééééééééééééééééé";
        assert_eq!(name.chars().count(), SHORT);
        assert_eq!(short_title(None, name), name);
    }

    #[test]
    fn an_empty_name_removes_the_given_one() {
        let mut prefs = Prefs::new();
        set_label(&mut prefs, "id-1", "  router  ");
        assert_eq!(prefs["id-1"].label.as_deref(), Some("router"));
        set_label(&mut prefs, "id-1", " ");
        assert!(prefs.is_empty());
    }
}
