use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    Account,
    Directory,
    Name,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Request {
    pub account: String,
    pub directory: PathBuf,
    pub name: String,
}

#[derive(Debug, PartialEq, Eq)]
pub enum FormOutcome {
    Continue,
    Cancel,
    Submit(Request),
}

#[derive(Debug)]
pub struct NewSessionForm {
    pub accounts: Vec<String>,
    pub account: usize,
    pub directory: String,
    pub name: String,
    pub field: Field,
    pub error: Option<String>,
}

impl NewSessionForm {
    pub fn new(accounts: Vec<String>, directory: &Path) -> Self {
        Self {
            accounts,
            account: 0,
            directory: directory.display().to_string(),
            name: String::new(),
            field: Field::Account,
            error: None,
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent, home: &Path) -> FormOutcome {
        let plain = !key.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT);
        match key.code {
            KeyCode::Esc => FormOutcome::Cancel,
            KeyCode::Char('c') if key.modifiers == KeyModifiers::CONTROL => FormOutcome::Cancel,
            KeyCode::Tab | KeyCode::Down => self.move_field(1),
            KeyCode::BackTab | KeyCode::Up => self.move_field(2),
            KeyCode::Enter => self.submit(home),
            KeyCode::Left if self.field == Field::Account => {
                self.cycle_account(self.accounts.len().saturating_sub(1))
            },
            KeyCode::Right if self.field == Field::Account => self.cycle_account(1),
            KeyCode::Backspace => {
                if let Some(text) = self.text_mut() {
                    text.pop();
                }
                FormOutcome::Continue
            },
            KeyCode::Char(c) if plain => {
                if let Some(text) = self.text_mut() {
                    text.push(c);
                }
                FormOutcome::Continue
            },
            _ => FormOutcome::Continue,
        }
    }

    fn text_mut(&mut self) -> Option<&mut String> {
        match self.field {
            Field::Account => None,
            Field::Directory => Some(&mut self.directory),
            Field::Name => Some(&mut self.name),
        }
    }

    fn move_field(&mut self, steps: usize) -> FormOutcome {
        let order = [Field::Account, Field::Directory, Field::Name];
        let current = order.iter().position(|field| *field == self.field).unwrap_or(0);
        self.field = order[(current + steps) % order.len()];
        FormOutcome::Continue
    }

    fn cycle_account(&mut self, steps: usize) -> FormOutcome {
        if !self.accounts.is_empty() {
            self.account = (self.account + steps) % self.accounts.len();
        }
        FormOutcome::Continue
    }

    fn submit(&mut self, home: &Path) -> FormOutcome {
        let Some(account) = self.accounts.get(self.account) else {
            self.error = Some("no accounts: claude-accounts add <name>".into());
            return FormOutcome::Continue;
        };
        let directory = expand_home(self.directory.trim(), home);
        if !directory.is_dir() {
            self.error = Some(format!("no such directory: {}", directory.display()));
            return FormOutcome::Continue;
        }
        FormOutcome::Submit(Request {
            account: account.clone(),
            directory,
            name: self.name.trim().to_string(),
        })
    }
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

/// Starts a background session and returns its short id.
pub fn launch(request: &Request) -> Result<String, String> {
    let mut command = Command::new("claude-accounts");
    command.args(["run", &request.account, "--bg"]);
    if !request.name.is_empty() {
        command.args(["-n", &request.name]);
    }
    let output = command
        .current_dir(&request.directory)
        .stdin(Stdio::null())
        .output()
        .map_err(|error| format!("claude-accounts: {error}"))?;
    let text = format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    if !output.status.success() {
        return Err(format!("cannot start the session: {}", text.trim()));
    }
    parse_backgrounded_id(&text).ok_or_else(|| format!("no session id in: {}", text.trim()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

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

    #[test]
    fn form_cycles_accounts_and_submits() {
        let home = std::env::temp_dir();
        let mut form = NewSessionForm::new(vec!["louis".into(), "lystriq".into()], &home);
        form.handle_key(key(KeyCode::Right), &home);
        form.handle_key(key(KeyCode::Tab), &home);
        form.handle_key(key(KeyCode::Tab), &home);
        for c in "api".chars() {
            form.handle_key(key(KeyCode::Char(c)), &home);
        }
        let outcome = form.handle_key(key(KeyCode::Enter), &home);
        assert_eq!(
            outcome,
            FormOutcome::Submit(Request { account: "lystriq".into(), directory: home.clone(), name: "api".into() })
        );
    }

    #[test]
    fn missing_directory_is_refused_in_the_form() {
        let home = std::env::temp_dir();
        let mut form = NewSessionForm::new(vec!["louis".into()], Path::new("/nonexistent/dir"));
        assert_eq!(form.handle_key(key(KeyCode::Enter), &home), FormOutcome::Continue);
        assert!(form.error.as_deref().unwrap_or_default().contains("no such directory"));
    }

    #[test]
    fn arrows_without_any_account_do_not_panic() {
        let home = std::env::temp_dir();
        let mut form = NewSessionForm::new(Vec::new(), &home);
        form.handle_key(key(KeyCode::Left), &home);
        form.handle_key(key(KeyCode::Right), &home);
        assert_eq!(form.handle_key(key(KeyCode::Enter), &home), FormOutcome::Continue);
        assert!(form.error.as_deref().unwrap_or_default().contains("no accounts"));
    }

    #[test]
    fn left_on_first_account_wraps_to_the_last() {
        let home = std::env::temp_dir();
        let mut form = NewSessionForm::new(vec!["a".into(), "b".into(), "c".into()], &home);
        form.handle_key(key(KeyCode::Left), &home);
        assert_eq!(form.account, 2);
    }
}
