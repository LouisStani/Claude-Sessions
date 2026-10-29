//! Keeps the session list current: refreshes when Claude Code or a mailbox writes
//! something, and every `FALLBACK` in case a change was missed.

use std::collections::HashMap;
use std::sync::mpsc::Sender;
use std::thread;
use std::time::Duration;

use crate::AppEvent;
use crate::accounts::{self, Snapshot};
use crate::mailbox::{self, Mailbox};
use crate::watcher::{self, ChangeWatcher, Wake};

const FALLBACK: Duration = Duration::from_secs(30);
const SETTLE: Duration = Duration::from_millis(300);
const WITHOUT_WATCHER: Duration = Duration::from_secs(10);

struct Refresher {
    watcher: Result<ChangeWatcher, String>,
    mailbox: Result<Mailbox, String>,
    watched_accounts: Vec<String>,
}

impl Refresher {
    fn new() -> Self {
        let mailbox = mailbox::default_root().map(Mailbox::new).and_then(|mailbox| {
            mailbox.create_root()?;
            Ok(mailbox)
        });
        Self {
            watcher: ChangeWatcher::new().map_err(|error| format!("file watching unavailable: {error}")),
            mailbox: mailbox.map_err(|error| format!("messages: {error}")),
            watched_accounts: Vec::new(),
        }
    }

    /// Account directories cost a `status --json` (about 1.3 s of CPU): only when accounts change.
    fn watch_accounts(&mut self, snapshot: &mut Snapshot) {
        let (Ok(watcher), Ok(mailbox)) = (&mut self.watcher, &self.mailbox) else {
            return;
        };
        if snapshot.accounts == self.watched_accounts {
            return;
        }
        match accounts::account_dirs() {
            Ok(dirs) => {
                snapshot.errors.extend(watcher.ensure(&watcher::targets(&dirs, mailbox.root())));
                self.watched_accounts = snapshot.accounts.clone();
            },
            Err(error) => snapshot.errors.push(format!("account directories: {error}")),
        }
    }

    fn unread_counts(&self, snapshot: &mut Snapshot) -> HashMap<String, usize> {
        let mailbox = match &self.mailbox {
            Ok(mailbox) => mailbox,
            Err(error) => {
                snapshot.errors.push(error.clone());
                return HashMap::new();
            },
        };
        snapshot
            .sessions
            .iter()
            .filter_map(|session| {
                let count = mailbox.unread_count(&session.session_id).ok()?;
                (count > 0).then(|| (session.session_id.clone(), count))
            })
            .collect()
    }

    fn refresh(&mut self) -> Result<(Snapshot, HashMap<String, usize>), String> {
        let mut snapshot = accounts::snapshot().map_err(|error| error.to_string())?;
        if let Err(error) = &self.watcher {
            snapshot.errors.push(error.clone());
        }
        self.watch_accounts(&mut snapshot);
        let unread = self.unread_counts(&mut snapshot);
        Ok((snapshot, unread))
    }

    fn wait(&self) {
        match &self.watcher {
            Ok(watcher) => {
                if let Wake::Closed = watcher.wait(FALLBACK, SETTLE) {
                    thread::sleep(WITHOUT_WATCHER);
                }
            },
            Err(_) => thread::sleep(WITHOUT_WATCHER),
        }
    }
}

pub fn spawn(events: Sender<AppEvent>) {
    thread::spawn(move || {
        let mut refresher = Refresher::new();
        loop {
            if events.send(AppEvent::Snapshot(refresher.refresh())).is_err() {
                return;
            }
            refresher.wait();
        }
    });
}
