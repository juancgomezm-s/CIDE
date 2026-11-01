use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use jc_app::{Key, KeyStroke, Modifiers};

pub fn is_action_event(event: KeyEvent) -> bool {
    event.kind != KeyEventKind::Release
}

pub fn to_stroke(event: KeyEvent) -> Option<KeyStroke> {
    let key = match event.code {
        KeyCode::Char(ch) => Key::Char(ch.to_ascii_lowercase()),
        KeyCode::F(number) => Key::Function(number),
        KeyCode::Tab => Key::Tab,
        KeyCode::BackTab => Key::Tab,
        KeyCode::Enter => Key::Enter,
        KeyCode::Esc => Key::Escape,
        KeyCode::Up => Key::Up,
        KeyCode::Down => Key::Down,
        KeyCode::Left => Key::Left,
        KeyCode::Right => Key::Right,
        KeyCode::PageUp => Key::PageUp,
        KeyCode::PageDown => Key::PageDown,
        KeyCode::Home => Key::Home,
        KeyCode::End => Key::End,
        _ => return None,
    };
    let mut modifiers = Modifiers::NONE;
    if event.modifiers.contains(KeyModifiers::CONTROL) {
        modifiers = modifiers.union(Modifiers::CTRL);
    }
    if event.modifiers.contains(KeyModifiers::ALT) {
        modifiers = modifiers.union(Modifiers::ALT);
    }
    if event.modifiers.contains(KeyModifiers::SHIFT) || matches!(event.code, KeyCode::BackTab) {
        modifiers = modifiers.union(Modifiers::SHIFT);
    }
    Some(KeyStroke::new(modifiers, key))
}

pub fn is_plain_text(event: KeyEvent) -> bool {
    matches!(event.code, KeyCode::Char(_))
        && !event.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ctrl_shortcuts_normalize_ascii_case() {
        let event = KeyEvent::new(KeyCode::Char('S'), KeyModifiers::CONTROL | KeyModifiers::SHIFT);
        let stroke = to_stroke(event).unwrap();
        assert_eq!(stroke.key, Key::Char('s'));
        assert!(stroke.modifiers.contains(Modifiers::CTRL));
        assert!(stroke.modifiers.contains(Modifiers::SHIFT));
    }

    #[test]
    fn backtab_is_shift_tab() {
        let stroke = to_stroke(KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT)).unwrap();
        assert_eq!(stroke.key, Key::Tab);
        assert!(stroke.modifiers.contains(Modifiers::SHIFT));
    }
}
