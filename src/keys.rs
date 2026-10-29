use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

const ESC: u8 = 0x1b;

pub fn key_bytes(key: KeyEvent, app_cursor: bool) -> Option<Vec<u8>> {
    let mods = key.modifiers;
    let alt = mods.contains(KeyModifiers::ALT);
    let bytes = match key.code {
        KeyCode::Char(c) if mods.contains(KeyModifiers::CONTROL) => vec![control_byte(c)?],
        KeyCode::Char(c) => c.to_string().into_bytes(),
        KeyCode::Enter => vec![b'\r'],
        KeyCode::Backspace => vec![0x7f],
        KeyCode::Tab => vec![b'\t'],
        KeyCode::BackTab => return Some(b"\x1b[Z".to_vec()),
        KeyCode::Esc => vec![ESC],
        KeyCode::Up => return Some(cursor_key(b'A', mods, app_cursor)),
        KeyCode::Down => return Some(cursor_key(b'B', mods, app_cursor)),
        KeyCode::Right => return Some(cursor_key(b'C', mods, app_cursor)),
        KeyCode::Left => return Some(cursor_key(b'D', mods, app_cursor)),
        KeyCode::Home => return Some(cursor_key(b'H', mods, app_cursor)),
        KeyCode::End => return Some(cursor_key(b'F', mods, app_cursor)),
        KeyCode::Insert => return Some(tilde_key(2, mods)),
        KeyCode::Delete => return Some(tilde_key(3, mods)),
        KeyCode::PageUp => return Some(tilde_key(5, mods)),
        KeyCode::PageDown => return Some(tilde_key(6, mods)),
        KeyCode::F(n) => return function_key(n, mods),
        _ => return None,
    };
    Some(if alt { prefixed_with_esc(bytes) } else { bytes })
}

pub fn paste_bytes(text: &str, bracketed: bool) -> Vec<u8> {
    if !bracketed {
        return text.as_bytes().to_vec();
    }
    [b"\x1b[200~".as_slice(), text.as_bytes(), b"\x1b[201~"].concat()
}

fn control_byte(c: char) -> Option<u8> {
    match c {
        'a'..='z' | 'A'..='Z' => Some(c.to_ascii_lowercase() as u8 & 0x1f),
        ' ' | '@' => Some(0),
        '[' => Some(ESC),
        '\\' => Some(0x1c),
        ']' => Some(0x1d),
        _ => None,
    }
}

fn prefixed_with_esc(bytes: Vec<u8>) -> Vec<u8> {
    [vec![ESC], bytes].concat()
}

/// xterm modifier parameter: 1 + shift(1) + alt(2) + ctrl(4).
fn modifier_param(mods: KeyModifiers) -> u8 {
    let mut param = 1;
    if mods.contains(KeyModifiers::SHIFT) {
        param += 1;
    }
    if mods.contains(KeyModifiers::ALT) {
        param += 2;
    }
    if mods.contains(KeyModifiers::CONTROL) {
        param += 4;
    }
    param
}

fn cursor_key(letter: u8, mods: KeyModifiers, app_cursor: bool) -> Vec<u8> {
    let param = modifier_param(mods);
    if param > 1 {
        return format!("\x1b[1;{param}{}", letter as char).into_bytes();
    }
    let introducer = if app_cursor { b'O' } else { b'[' };
    vec![ESC, introducer, letter]
}

fn tilde_key(code: u8, mods: KeyModifiers) -> Vec<u8> {
    let param = modifier_param(mods);
    if param > 1 {
        return format!("\x1b[{code};{param}~").into_bytes();
    }
    format!("\x1b[{code}~").into_bytes()
}

fn function_key(n: u8, mods: KeyModifiers) -> Option<Vec<u8>> {
    let param = modifier_param(mods);
    let ss3 = match n {
        1 => Some(b'P'),
        2 => Some(b'Q'),
        3 => Some(b'R'),
        4 => Some(b'S'),
        _ => None,
    };
    if let Some(letter) = ss3 {
        if param > 1 {
            return Some(format!("\x1b[1;{param}{}", letter as char).into_bytes());
        }
        return Some(vec![ESC, b'O', letter]);
    }
    let code = match n {
        5 => 15,
        6 => 17,
        7 => 18,
        8 => 19,
        9 => 20,
        10 => 21,
        11 => 23,
        12 => 24,
        _ => return None,
    };
    Some(tilde_key(code, mods))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode, mods: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, mods)
    }

    fn plain(code: KeyCode) -> Option<Vec<u8>> {
        key_bytes(key(code, KeyModifiers::NONE), false)
    }

    #[test]
    fn text_and_basic_keys() {
        assert_eq!(plain(KeyCode::Char('é')), Some("é".as_bytes().to_vec()));
        assert_eq!(plain(KeyCode::Enter), Some(b"\r".to_vec()));
        assert_eq!(plain(KeyCode::Backspace), Some(b"\x7f".to_vec()));
        assert_eq!(plain(KeyCode::Esc), Some(b"\x1b".to_vec()));
        assert_eq!(plain(KeyCode::BackTab), Some(b"\x1b[Z".to_vec()));
    }

    #[test]
    fn control_and_alt() {
        let ctrl_c = key(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert_eq!(key_bytes(ctrl_c, false), Some(vec![0x03]));
        let alt_x = key(KeyCode::Char('x'), KeyModifiers::ALT);
        assert_eq!(key_bytes(alt_x, false), Some(b"\x1bx".to_vec()));
        let alt_enter = key(KeyCode::Enter, KeyModifiers::ALT);
        assert_eq!(key_bytes(alt_enter, false), Some(b"\x1b\r".to_vec()));
    }

    #[test]
    fn arrows_follow_app_cursor_mode() {
        let up = key(KeyCode::Up, KeyModifiers::NONE);
        assert_eq!(key_bytes(up, false), Some(b"\x1b[A".to_vec()));
        assert_eq!(key_bytes(up, true), Some(b"\x1bOA".to_vec()));
        let ctrl_left = key(KeyCode::Left, KeyModifiers::CONTROL);
        assert_eq!(key_bytes(ctrl_left, true), Some(b"\x1b[1;5D".to_vec()));
    }

    #[test]
    fn tilde_and_function_keys() {
        assert_eq!(plain(KeyCode::Delete), Some(b"\x1b[3~".to_vec()));
        assert_eq!(plain(KeyCode::F(1)), Some(b"\x1bOP".to_vec()));
        assert_eq!(plain(KeyCode::F(5)), Some(b"\x1b[15~".to_vec()));
        assert_eq!(plain(KeyCode::F(13)), None);
    }

    #[test]
    fn paste_is_bracketed_only_when_asked() {
        assert_eq!(paste_bytes("a\nb", false), b"a\nb".to_vec());
        assert_eq!(paste_bytes("a\nb", true), b"\x1b[200~a\nb\x1b[201~".to_vec());
    }
}
