use std::fs::{self, DirBuilder};
use std::io;
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

const READ_DIR: &str = "read";

static SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    pub from_session: String,
    pub from_title: String,
    pub from_account: String,
    pub sent_at: u64,
    pub text: String,
}

pub struct Mailbox {
    root: PathBuf,
}

/// `$XDG_DATA_HOME/claude-sessions/mailbox`, else `~/.local/share/claude-sessions/mailbox`.
pub fn default_root() -> io::Result<PathBuf> {
    let data = match std::env::var_os("XDG_DATA_HOME").filter(|value| !value.is_empty()) {
        Some(data) => PathBuf::from(data),
        None => std::env::home_dir()
            .ok_or_else(|| io::Error::other("HOME is not set"))?
            .join(".local/share"),
    };
    Ok(data.join("claude-sessions/mailbox"))
}

pub fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |elapsed| elapsed.as_secs())
}

/// Session ids become directory names: only UUID-like ids are accepted.
fn is_valid_session_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

fn private_dir(path: &Path) -> io::Result<()> {
    DirBuilder::new().recursive(true).mode(0o700).create(path)
}

fn is_message_file(path: &Path) -> bool {
    path.extension().is_some_and(|ext| ext == "json")
        && path.file_name().is_some_and(|name| !name.to_string_lossy().starts_with('.'))
}

impl Mailbox {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Creates the root (700) so it can be watched before the first message.
    pub fn create_root(&self) -> io::Result<()> {
        private_dir(&self.root)
    }

    fn inbox(&self, session: &str) -> io::Result<PathBuf> {
        if !is_valid_session_id(session) {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, format!("invalid session id: {session:?}")));
        }
        Ok(self.root.join(session))
    }

    /// Writes to a hidden temporary file first, then renames: a reader never sees half a message.
    pub fn send(&self, to_session: &str, message: &Message) -> io::Result<()> {
        let inbox = self.inbox(to_session)?;
        private_dir(&inbox)?;
        let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let stem = format!("{:020}-{}-{sequence}", message.sent_at, std::process::id());
        let temporary = inbox.join(format!(".{stem}.tmp"));
        fs::write(&temporary, serde_json::to_vec(message).map_err(io::Error::other)?)?;
        fs::rename(&temporary, inbox.join(format!("{stem}.json")))
    }

    fn unread_files(&self, session: &str) -> io::Result<Vec<PathBuf>> {
        let inbox = self.inbox(session)?;
        let entries = match fs::read_dir(&inbox) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error),
        };
        let mut files = Vec::new();
        for entry in entries {
            let path = entry?.path();
            if path.is_file() && is_message_file(&path) {
                files.push(path);
            }
        }
        files.sort();
        Ok(files)
    }

    pub fn unread_count(&self, session: &str) -> io::Result<usize> {
        Ok(self.unread_files(session)?.len())
    }

    /// Returns unread messages, oldest first, and moves them to `read/`.
    pub fn take_unread(&self, session: &str) -> io::Result<Vec<Message>> {
        let files = self.unread_files(session)?;
        if files.is_empty() {
            return Ok(Vec::new());
        }
        let read_dir = self.inbox(session)?.join(READ_DIR);
        private_dir(&read_dir)?;
        let mut messages = Vec::with_capacity(files.len());
        for file in files {
            let message = serde_json::from_slice(&fs::read(&file)?).map_err(io::Error::other)?;
            let name = file.file_name().ok_or_else(|| io::Error::other("message file without name"))?;
            fs::rename(&file, read_dir.join(name))?;
            messages.push(message);
        }
        Ok(messages)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn temp_root(label: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("claude-sessions-test-{label}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        root
    }

    fn message(text: &str, sent_at: u64) -> Message {
        Message {
            from_session: "aaaa-1111".into(),
            from_title: "roblox".into(),
            from_account: "louis".into(),
            sent_at,
            text: text.into(),
        }
    }

    #[test]
    fn send_then_take_returns_messages_in_order_once() {
        let root = temp_root("order");
        let mailbox = Mailbox::new(root.clone());
        mailbox.send("bbbb-2222", &message("second", 20)).unwrap();
        mailbox.send("bbbb-2222", &message("first", 10)).unwrap();
        assert_eq!(mailbox.unread_count("bbbb-2222").unwrap(), 2);
        let texts: Vec<_> = mailbox.take_unread("bbbb-2222").unwrap().into_iter().map(|m| m.text).collect();
        assert_eq!(texts, ["first", "second"]);
        assert_eq!(mailbox.unread_count("bbbb-2222").unwrap(), 0);
        assert!(mailbox.take_unread("bbbb-2222").unwrap().is_empty());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn inbox_is_private_and_temporary_files_are_ignored() {
        let root = temp_root("private");
        let mailbox = Mailbox::new(root.clone());
        mailbox.send("cccc", &message("hi", 1)).unwrap();
        let mode = fs::metadata(root.join("cccc")).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o700);
        fs::write(root.join("cccc/.half.tmp"), b"{").unwrap();
        fs::write(root.join("cccc/.hidden.json"), b"{").unwrap();
        fs::write(root.join("cccc/stray.txt"), b"{").unwrap();
        assert_eq!(mailbox.unread_count("cccc").unwrap(), 1);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn unknown_session_has_no_messages_and_bad_ids_are_refused() {
        let mailbox = Mailbox::new(temp_root("ids"));
        assert_eq!(mailbox.unread_count("never-written").unwrap(), 0);
        assert!(mailbox.send("../escape", &message("x", 1)).is_err());
        assert!(mailbox.unread_count("a/b").is_err());
        assert!(mailbox.send("", &message("x", 1)).is_err());
    }
}
