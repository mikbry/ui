//! Backend-agnostic input events.
//!
//! Each backend translates its native event stream (DOM events, crossterm
//! `KeyEvent`, winit window events, ...) into the shared [`InputEvent`]
//! contract before dispatching to headless components. This keeps event
//! handling code in headless components independent of any backend.

/// A logical key in the shared event model.
///
/// Backends are expected to map their raw key codes into these variants.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Key {
    Char(char),
    Enter,
    Space,
    Tab,
    Escape,
    Backspace,
    Delete,
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    PageUp,
    PageDown,
    Other(String),
}

impl Key {
    /// Whether this key triggers activation (Enter or Space).
    pub fn is_activation(&self) -> bool {
        matches!(self, Key::Enter | Key::Space)
    }

    /// Logical string identifier used by headless components that already
    /// expose `&str`-based handlers.
    ///
    /// Returns `Some(&str)` for every named key, for printable ASCII chars,
    /// and for `'\n'` / `'\t'`. Returns `None` for `Char(c)` when `c` is
    /// outside that set — non-ASCII or control characters do not have a
    /// stable static representation, and callers must handle that case
    /// explicitly rather than receive a misleading empty string.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Key::Char(c) => char_as_str(*c),
            Key::Enter => Some("Enter"),
            Key::Space => Some(" "),
            Key::Tab => Some("Tab"),
            Key::Escape => Some("Escape"),
            Key::Backspace => Some("Backspace"),
            Key::Delete => Some("Delete"),
            Key::Left => Some("ArrowLeft"),
            Key::Right => Some("ArrowRight"),
            Key::Up => Some("ArrowUp"),
            Key::Down => Some("ArrowDown"),
            Key::Home => Some("Home"),
            Key::End => Some("End"),
            Key::PageUp => Some("PageUp"),
            Key::PageDown => Some("PageDown"),
            Key::Other(s) => Some(s.as_str()),
        }
    }
}

// Printable ASCII chars (0x20..=0x7e), one byte each. Indexing a `&'static
// str` by a single-byte ASCII range yields a `&'static str`, so we can hand
// out a static slice without allocating.
const PRINTABLE_ASCII: &str =
    " !\"#$%&'()*+,-./0123456789:;<=>?@ABCDEFGHIJKLMNOPQRSTUVWXYZ[\\]^_`abcdefghijklmnopqrstuvwxyz{|}~";

fn char_as_str(c: char) -> Option<&'static str> {
    match c {
        '\n' => Some("\n"),
        '\t' => Some("\t"),
        c if (' '..='~').contains(&c) => {
            let idx = (c as u8 - b' ') as usize;
            Some(&PRINTABLE_ASCII[idx..idx + 1])
        }
        _ => None,
    }
}

/// Pointer / mouse button identifier.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum PointerButton {
    Primary,
    Secondary,
    Middle,
}

/// Shared input event. Backends emit these; headless components consume them.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum InputEvent {
    KeyDown(Key),
    KeyUp(Key),
    PointerDown(PointerButton),
    PointerUp(PointerButton),
    Focus,
    Blur,
}

/// Outcome of routing one host-forwarded event through mkui's input router.
///
/// An embedding host (e.g. a `wgpu` game forwarding `winit` events into
/// mkui, per the embedding contract tracked at #173/#176) needs a per-event
/// answer to "did mkui want this?" so it knows whether to also feed the
/// event to its own gameplay input (movement, camera look, hotkeys, ...).
/// `Routed` is that answer. Each backend's concrete router (hit-testing,
/// pointer capture, keyboard focus) decides the value; this type is just
/// the shared vocabulary so every backend's router speaks the same
/// consumed/not-consumed contract.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Routed {
    /// mkui consumed the event. The host must not also act on it — e.g. a
    /// click that landed on a HUD button, or a key swallowed by a focused
    /// text field.
    Consumed,
    /// mkui had no interest in the event. The host's own input layer is
    /// free to process it as usual. The default: absent any other signal,
    /// an event is the host's.
    #[default]
    Ignored,
}

impl Routed {
    /// Build a `Routed` from a boolean "did mkui want this" predicate.
    /// Lets callers write `Routed::from_consumed(hit.is_some())` instead of
    /// a manual `if`.
    pub fn from_consumed(consumed: bool) -> Self {
        if consumed {
            Routed::Consumed
        } else {
            Routed::Ignored
        }
    }

    pub fn is_consumed(self) -> bool {
        matches!(self, Routed::Consumed)
    }

    pub fn is_ignored(self) -> bool {
        matches!(self, Routed::Ignored)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enter_and_space_are_activation_keys() {
        assert!(Key::Enter.is_activation());
        assert!(Key::Space.is_activation());
        assert!(!Key::Tab.is_activation());
        assert!(!Key::Char('a').is_activation());
    }

    #[test]
    fn key_as_str_matches_headless_expectations() {
        // Headless components key off the literal " " and "Enter" strings.
        assert_eq!(Key::Space.as_str(), Some(" "));
        assert_eq!(Key::Enter.as_str(), Some("Enter"));
    }

    #[test]
    fn printable_ascii_chars_round_trip_through_as_str() {
        assert_eq!(Key::Char('a').as_str(), Some("a"));
        assert_eq!(Key::Char('Z').as_str(), Some("Z"));
        assert_eq!(Key::Char('5').as_str(), Some("5"));
        assert_eq!(Key::Char(' ').as_str(), Some(" "));
        assert_eq!(Key::Char('~').as_str(), Some("~"));
    }

    #[test]
    fn newline_and_tab_chars_are_supported() {
        assert_eq!(Key::Char('\n').as_str(), Some("\n"));
        assert_eq!(Key::Char('\t').as_str(), Some("\t"));
    }

    #[test]
    fn non_ascii_and_control_chars_return_none_instead_of_lying() {
        // Previously these returned "" — a silent empty string that callers
        // could not distinguish from a legitimate result.
        assert_eq!(Key::Char('é').as_str(), None);
        assert_eq!(Key::Char('🦀').as_str(), None);
        assert_eq!(Key::Char('\x07').as_str(), None); // bell
        assert_eq!(Key::Char('\x1b').as_str(), None); // escape
    }

    #[test]
    fn routed_from_consumed_round_trips() {
        assert_eq!(Routed::from_consumed(true), Routed::Consumed);
        assert_eq!(Routed::from_consumed(false), Routed::Ignored);
    }

    #[test]
    fn routed_predicates_agree_with_the_variant() {
        assert!(Routed::Consumed.is_consumed());
        assert!(!Routed::Consumed.is_ignored());
        assert!(Routed::Ignored.is_ignored());
        assert!(!Routed::Ignored.is_consumed());
    }
}
