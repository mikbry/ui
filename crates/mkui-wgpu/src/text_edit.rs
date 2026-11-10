//! `TextEditState` — single-line text-editing state: caret, selection, and
//! IME composition (#185).
//!
//! Pure logic, no winit/host dependency and no OS clipboard access: like
//! [`crate::pointer::PointerState`], one instance is owned by the caller and
//! kept across frames (the renderer is immediate-mode and retains no
//! component tree between frames). [`TextEditState::copy`] / [`TextEditState::cut`] hand
//! back an owned `String` for a host-provided clipboard (#206) to store;
//! [`TextEditState::paste`] takes an owned `&str` the host supplies. Nothing in this module
//! touches an OS API, so it slots into #206's trait once that lands without
//! any change here.
//!
//! Positions (`caret`, selection bounds, composing range) are byte offsets
//! into [`TextEditState::text`], always on a UTF-8 char boundary. Movement
//! steps by `char`, not by byte, so multi-byte scalars (accents, CJK) move
//! correctly in one key press. Grapheme-cluster-aware movement (so e.g. a
//! base character + combining marks, or an emoji ZWJ sequence, moves as one
//! unit) is out of scope here — deferred to whichever issue adds a
//! grapheme-segmentation dependency.
//!
//! [`TextEditState`] only tracks logical state; it has no pixel/x-position
//! concept and no fixed-width character count. Caret/selection pixel
//! placement for rendering uses an approximate advance width
//! (`crate::components::text_field::text_field_editable`) pending #182's
//! glyph-measurement API — that's a rendering-layer approximation the
//! logical model here doesn't need to know about.

/// Cursor/selection/composition state for a single-line editable text field.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TextEditState {
    text: String,
    /// Byte offset, always on a char boundary.
    caret: usize,
    /// Byte offset of the selection anchor. `None` means no selection;
    /// collapses to `None` whenever `anchor == caret`.
    anchor: Option<usize>,
    /// Byte range `[start, end)` of IME preedit text currently inlined into
    /// `text`, while a composition is in progress.
    composing: Option<(usize, usize)>,
}

impl TextEditState {
    /// New state with `initial` as the text and the caret at its end.
    pub fn new(initial: impl Into<String>) -> Self {
        let text = initial.into();
        let caret = text.len();
        Self {
            text,
            caret,
            anchor: None,
            composing: None,
        }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn caret(&self) -> usize {
        self.caret
    }

    /// Normalized `(start, end)` with `start <= end`, or `None` when there's
    /// no selection (anchor absent, or coincides with the caret).
    pub fn selection(&self) -> Option<(usize, usize)> {
        let anchor = self.anchor?;
        if anchor == self.caret {
            return None;
        }
        Some(if anchor < self.caret {
            (anchor, self.caret)
        } else {
            (self.caret, anchor)
        })
    }

    /// Byte range `[start, end)` of the in-progress IME composition, if any.
    pub fn composing(&self) -> Option<(usize, usize)> {
        self.composing
    }

    pub fn is_composing(&self) -> bool {
        self.composing.is_some()
    }

    fn set_caret(&mut self, pos: usize, extend_selection: bool) {
        if extend_selection {
            if self.anchor.is_none() {
                self.anchor = Some(self.caret);
            }
        } else {
            self.anchor = None;
        }
        self.caret = pos;
        // A shift+arrow move back to the anchor must drop the anchor, not
        // just leave it coincident with the caret — otherwise it resurfaces
        // as a selection (with stale byte offsets) once a later edit moves
        // the caret away again. See the #228 review regression tests.
        if self.anchor == Some(self.caret) {
            self.anchor = None;
        }
    }

    /// Move the caret one char left. A plain (non-extending) move with an
    /// active selection collapses to the selection start instead of moving
    /// one further char, matching standard text-field behavior.
    pub fn move_left(&mut self, extend_selection: bool) {
        if !extend_selection {
            if let Some((start, _)) = self.selection() {
                self.anchor = None;
                self.caret = start;
                return;
            }
        }
        let pos = prev_char_boundary(&self.text, self.caret);
        self.set_caret(pos, extend_selection);
    }

    /// Move the caret one char right. See [`Self::move_left`] for the
    /// collapse-on-plain-move-with-selection behavior.
    pub fn move_right(&mut self, extend_selection: bool) {
        if !extend_selection {
            if let Some((_, end)) = self.selection() {
                self.anchor = None;
                self.caret = end;
                return;
            }
        }
        let pos = next_char_boundary(&self.text, self.caret);
        self.set_caret(pos, extend_selection);
    }

    pub fn move_home(&mut self, extend_selection: bool) {
        self.set_caret(0, extend_selection);
    }

    pub fn move_end(&mut self, extend_selection: bool) {
        self.set_caret(self.text.len(), extend_selection);
    }

    pub fn select_all(&mut self) {
        self.anchor = Some(0);
        self.caret = self.text.len();
        if self.anchor == Some(self.caret) {
            self.anchor = None;
        }
    }

    pub fn clear_selection(&mut self) {
        self.anchor = None;
    }

    /// Delete the current selection, if any, collapsing the caret to its
    /// start. Returns the removed text.
    fn delete_selection(&mut self) -> Option<String> {
        let (start, end) = self.selection()?;
        let removed = self.text[start..end].to_string();
        self.text.replace_range(start..end, "");
        self.caret = start;
        self.anchor = None;
        Some(removed)
    }

    /// Insert `s` at the caret, replacing the selection if any. The caret
    /// lands just after the inserted text. Line breaks are stripped — this
    /// is a single-line field, so pasted or typed multi-line text is
    /// flattened onto one line rather than silently violating the
    /// single-line caret/selection rendering contract.
    pub fn insert_str(&mut self, s: &str) {
        self.end_composing_in_place();
        let s = strip_line_breaks(s);
        self.delete_selection();
        self.text.insert_str(self.caret, &s);
        self.caret += s.len();
    }

    pub fn insert_char(&mut self, c: char) {
        let mut buf = [0u8; 4];
        self.insert_str(c.encode_utf8(&mut buf));
    }

    /// Delete the selection, or the char before the caret.
    pub fn backspace(&mut self) {
        self.end_composing_in_place();
        if self.delete_selection().is_some() {
            return;
        }
        if self.caret == 0 {
            return;
        }
        let start = prev_char_boundary(&self.text, self.caret);
        self.text.replace_range(start..self.caret, "");
        self.caret = start;
    }

    /// Delete the selection, or the char after the caret.
    pub fn delete(&mut self) {
        self.end_composing_in_place();
        if self.delete_selection().is_some() {
            return;
        }
        if self.caret == self.text.len() {
            return;
        }
        let end = next_char_boundary(&self.text, self.caret);
        self.text.replace_range(self.caret..end, "");
    }

    /// The selected text, without mutating. `None` if there's no selection.
    /// The host-provided clipboard (#206) stores the result; this never
    /// touches an OS clipboard itself.
    pub fn copy(&self) -> Option<String> {
        let (start, end) = self.selection()?;
        Some(self.text[start..end].to_string())
    }

    /// Remove and return the selected text, for the host clipboard (#206).
    pub fn cut(&mut self) -> Option<String> {
        self.delete_selection()
    }

    /// Insert host-supplied clipboard text at the caret, replacing the
    /// selection. `mkui` never reads the OS clipboard itself (#206) — the
    /// host hands the string in after reading it.
    pub fn paste(&mut self, s: &str) {
        self.insert_str(s);
    }

    /// Begin or update an IME composition: replaces any in-progress preedit
    /// with `preedit`, inserting at the caret (replacing the selection, if
    /// any) when no composition was active yet. Mirrors
    /// `winit::event::Ime::Preedit(text, cursor_range)` — `cursor` is the
    /// caret position *within* `preedit`, in bytes, defaulting to
    /// `preedit.len()` when the host doesn't supply one.
    pub fn set_composing(&mut self, preedit: &str, cursor: Option<usize>) {
        let preedit = strip_line_breaks(preedit);
        let start = if let Some((start, end)) = self.composing.take() {
            self.text.replace_range(start..end, "");
            start
        } else {
            self.delete_selection();
            self.caret
        };
        self.text.insert_str(start, &preedit);
        let end = start + preedit.len();
        self.composing = Some((start, end));
        let cursor = cursor.unwrap_or(preedit.len()).min(preedit.len());
        self.caret = start + cursor;
    }

    /// Commit the in-progress composition as final text — or, for an IME
    /// that commits directly without a preedit phase, insert `commit_text`
    /// at the caret. Clears composing state; the caret lands just after the
    /// committed text.
    pub fn commit_composing(&mut self, commit_text: &str) {
        if let Some((start, end)) = self.composing.take() {
            let commit_text = strip_line_breaks(commit_text);
            self.text.replace_range(start..end, &commit_text);
            self.caret = start + commit_text.len();
        } else {
            self.insert_str(commit_text);
        }
    }

    /// Cancel an in-progress composition, removing the preedit text
    /// entirely and restoring the caret to where composition started.
    /// No-op if no composition is in progress.
    pub fn cancel_composing(&mut self) {
        if let Some((start, end)) = self.composing.take() {
            self.text.replace_range(start..end, "");
            self.caret = start;
        }
    }

    /// Resolve an in-progress IME composition in place before any edit that
    /// isn't itself part of the composition lifecycle: stop tracking its
    /// range (the preedit text already live in `text` just becomes ordinary
    /// committed text). Without this, an intervening insert/backspace/delete
    /// can move the caret or change `text`'s length while a stale
    /// `composing` range still points at the old positions, so a later
    /// `commit_composing`/`cancel_composing` slices out of bounds and
    /// panics.
    fn end_composing_in_place(&mut self) {
        self.composing = None;
    }
}

/// Strip `\r` and `\n` from `s`. [`TextEditState`] is a single-line field —
/// both its logical model (one `caret`/selection along one axis) and
/// [`crate::components::text_field::text_field_editable`]'s rendering
/// assume text never breaks across lines, so any line breaks arriving via
/// paste, typed input, or IME commit/preedit are flattened onto one line
/// rather than silently producing a caret model the renderer can't place.
fn strip_line_breaks(s: &str) -> std::borrow::Cow<'_, str> {
    if s.contains(['\r', '\n']) {
        std::borrow::Cow::Owned(s.chars().filter(|c| *c != '\r' && *c != '\n').collect())
    } else {
        std::borrow::Cow::Borrowed(s)
    }
}

/// The byte offset of the char immediately before `byte_idx` (which must be
/// a valid char boundary), or `0` at the start of the string.
fn prev_char_boundary(s: &str, byte_idx: usize) -> usize {
    if byte_idx == 0 {
        return 0;
    }
    s[..byte_idx]
        .char_indices()
        .next_back()
        .map(|(i, _)| i)
        .unwrap_or(0)
}

/// The byte offset of the char immediately after `byte_idx` (which must be a
/// valid char boundary), or `s.len()` at the end of the string.
fn next_char_boundary(s: &str, byte_idx: usize) -> usize {
    s[byte_idx..]
        .char_indices()
        .nth(1)
        .map(|(i, _)| byte_idx + i)
        .unwrap_or(s.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_places_caret_at_end() {
        let state = TextEditState::new("hello");
        assert_eq!(state.text(), "hello");
        assert_eq!(state.caret(), 5);
        assert_eq!(state.selection(), None);
    }

    #[test]
    fn move_left_right_basic() {
        let mut state = TextEditState::new("abc");
        state.move_left(false);
        assert_eq!(state.caret(), 2);
        state.move_left(false);
        state.move_left(false);
        assert_eq!(state.caret(), 0);
        // Stays clamped at the start.
        state.move_left(false);
        assert_eq!(state.caret(), 0);

        state.move_right(false);
        state.move_right(false);
        state.move_right(false);
        assert_eq!(state.caret(), 3);
        // Stays clamped at the end.
        state.move_right(false);
        assert_eq!(state.caret(), 3);
    }

    #[test]
    fn move_left_right_steps_one_char_not_one_byte_on_multibyte_text() {
        // "日本語" is three chars, each 3 UTF-8 bytes (9 bytes total).
        let mut state = TextEditState::new("日本語");
        assert_eq!(state.caret(), 9);
        state.move_left(false);
        assert_eq!(state.caret(), 6);
        state.move_left(false);
        assert_eq!(state.caret(), 3);
        state.move_right(false);
        assert_eq!(state.caret(), 6);
    }

    #[test]
    fn shift_left_extends_selection_then_plain_left_collapses_to_start() {
        let mut state = TextEditState::new("hello");
        state.move_left(true);
        state.move_left(true);
        assert_eq!(state.selection(), Some((3, 5)));
        // Plain (non-shift) left collapses to the selection start rather
        // than moving one further char.
        state.move_left(false);
        assert_eq!(state.caret(), 3);
        assert_eq!(state.selection(), None);
    }

    #[test]
    fn shift_right_extends_selection_then_plain_right_collapses_to_end() {
        let mut state = TextEditState::new("hello");
        state.move_home(false);
        state.move_right(true);
        state.move_right(true);
        assert_eq!(state.selection(), Some((0, 2)));
        state.move_right(false);
        assert_eq!(state.caret(), 2);
        assert_eq!(state.selection(), None);
    }

    #[test]
    fn shift_arrow_back_to_anchor_clears_selection_highlight() {
        let mut state = TextEditState::new("hello");
        state.move_left(true);
        assert!(state.selection().is_some());
        state.move_right(true);
        assert_eq!(state.selection(), None, "anchor == caret collapses");
    }

    #[test]
    fn home_end_move_without_extending_by_default() {
        let mut state = TextEditState::new("hello");
        state.move_left(false);
        state.move_home(false);
        assert_eq!(state.caret(), 0);
        assert_eq!(state.selection(), None);
        state.move_end(false);
        assert_eq!(state.caret(), 5);
        assert_eq!(state.selection(), None);
    }

    #[test]
    fn shift_home_end_select_to_bounds() {
        let mut state = TextEditState::new("hello");
        state.move_home(false);
        state.move_end(true);
        assert_eq!(state.selection(), Some((0, 5)));
        state.clear_selection();
        state.move_end(false);
        state.move_home(true);
        assert_eq!(state.selection(), Some((0, 5)));
    }

    #[test]
    fn select_all_selects_everything() {
        let mut state = TextEditState::new("hello");
        state.move_left(false);
        state.select_all();
        assert_eq!(state.selection(), Some((0, 5)));
        assert_eq!(state.caret(), 5);
    }

    #[test]
    fn insert_char_at_caret_without_selection() {
        let mut state = TextEditState::new("helo");
        state.move_left(false);
        state.insert_char('l');
        assert_eq!(state.text(), "hello");
        assert_eq!(state.caret(), 4);
    }

    #[test]
    fn insert_str_replaces_selection() {
        let mut state = TextEditState::new("hello world");
        state.move_home(false);
        for _ in 0..5 {
            state.move_right(true);
        }
        assert_eq!(state.selection(), Some((0, 5)));
        state.insert_str("goodbye");
        assert_eq!(state.text(), "goodbye world");
        assert_eq!(state.caret(), 7);
        assert_eq!(state.selection(), None);
    }

    #[test]
    fn backspace_deletes_selection_when_present() {
        let mut state = TextEditState::new("hello");
        state.move_home(false);
        state.move_right(true);
        state.move_right(true);
        state.backspace();
        assert_eq!(state.text(), "llo");
        assert_eq!(state.caret(), 0);
        assert_eq!(state.selection(), None);
    }

    #[test]
    fn backspace_deletes_prev_char_when_no_selection() {
        let mut state = TextEditState::new("hello");
        state.backspace();
        assert_eq!(state.text(), "hell");
        assert_eq!(state.caret(), 4);
    }

    #[test]
    fn backspace_at_start_is_a_noop() {
        let mut state = TextEditState::new("hello");
        state.move_home(false);
        state.backspace();
        assert_eq!(state.text(), "hello");
        assert_eq!(state.caret(), 0);
    }

    #[test]
    fn delete_removes_selection_when_present() {
        let mut state = TextEditState::new("hello");
        state.move_home(false);
        state.move_right(true);
        state.move_right(true);
        state.delete();
        assert_eq!(state.text(), "llo");
        assert_eq!(state.caret(), 0);
    }

    #[test]
    fn delete_removes_next_char_when_no_selection() {
        let mut state = TextEditState::new("hello");
        state.move_home(false);
        state.delete();
        assert_eq!(state.text(), "ello");
        assert_eq!(state.caret(), 0);
    }

    #[test]
    fn delete_at_end_is_a_noop() {
        let mut state = TextEditState::new("hello");
        state.delete();
        assert_eq!(state.text(), "hello");
        assert_eq!(state.caret(), 5);
    }

    #[test]
    fn copy_returns_selection_without_mutating() {
        let mut state = TextEditState::new("hello world");
        state.move_home(false);
        for _ in 0..5 {
            state.move_right(true);
        }
        assert_eq!(state.copy(), Some("hello".to_string()));
        assert_eq!(state.text(), "hello world", "copy must not mutate");
        assert_eq!(state.selection(), Some((0, 5)), "copy must not clear it");
    }

    #[test]
    fn copy_with_no_selection_returns_none() {
        let state = TextEditState::new("hello");
        assert_eq!(state.copy(), None);
    }

    #[test]
    fn cut_removes_and_returns_selection() {
        let mut state = TextEditState::new("hello world");
        state.move_home(false);
        for _ in 0..5 {
            state.move_right(true);
        }
        assert_eq!(state.cut(), Some("hello".to_string()));
        assert_eq!(state.text(), " world");
        assert_eq!(state.caret(), 0);
    }

    #[test]
    fn paste_inserts_host_supplied_text_replacing_selection() {
        let mut state = TextEditState::new("hello world");
        state.move_home(false);
        for _ in 0..5 {
            state.move_right(true);
        }
        state.paste("goodbye");
        assert_eq!(state.text(), "goodbye world");
    }

    #[test]
    fn ime_composition_lifecycle_commit() {
        let mut state = TextEditState::new("ab");
        state.move_left(false); // caret between 'a' and 'b'
        state.set_composing("n", None);
        assert_eq!(state.text(), "anb");
        assert!(state.is_composing());
        assert_eq!(state.composing(), Some((1, 2)));

        // IME updates the preedit as the user keeps typing — must replace,
        // not append.
        state.set_composing("ni", None);
        assert_eq!(state.text(), "anib");
        assert_eq!(state.composing(), Some((1, 3)));

        // Commit finalizes the composed text (e.g. IME picks a kanji).
        state.commit_composing("んい");
        assert_eq!(state.text(), "aんいb");
        assert!(!state.is_composing());
        assert_eq!(state.composing(), None);
    }

    #[test]
    fn ime_composition_cancel_removes_preedit_and_restores_caret() {
        let mut state = TextEditState::new("ab");
        state.move_left(false);
        state.set_composing("xyz", None);
        assert_eq!(state.text(), "axyzb");
        state.cancel_composing();
        assert_eq!(state.text(), "ab");
        assert_eq!(state.caret(), 1);
        assert!(!state.is_composing());
    }

    #[test]
    fn ime_composing_replaces_selection() {
        let mut state = TextEditState::new("hello");
        state.move_home(false);
        for _ in 0..5 {
            state.move_right(true);
        }
        state.set_composing("n", None);
        assert_eq!(state.text(), "n");
        assert_eq!(state.composing(), Some((0, 1)));
    }

    #[test]
    fn ime_preedit_cursor_places_caret_within_preedit() {
        let mut state = TextEditState::new("");
        state.set_composing("abc", Some(1));
        assert_eq!(state.caret(), 1);
    }

    #[test]
    fn ime_commit_without_prior_preedit_inserts_directly() {
        let mut state = TextEditState::new("ab");
        state.move_left(false);
        state.commit_composing("X");
        assert_eq!(state.text(), "aXb");
        assert_eq!(state.caret(), 2);
    }

    #[test]
    fn shift_left_then_shift_right_back_to_anchor_does_not_resurrect_selection() {
        // Regression for #228 review P1: collapsing a selection back onto
        // its anchor via shift+arrow must drop the anchor, not just leave
        // it coincident with the caret.
        let mut state = TextEditState::new("abc");
        state.move_left(true);
        state.move_right(true);
        assert_eq!(state.selection(), None);

        state.insert_char('d');
        state.insert_char('e');
        assert_eq!(state.text(), "abcde");

        // Previously panicked: a stale anchor from the collapsed selection
        // above would be reinterpreted as a real selection once the caret
        // moved away, and `delete_selection` would slice past the now-
        // shorter string.
        state.backspace();
        state.backspace();
        assert_eq!(state.text(), "abc");
    }

    #[test]
    fn backspace_during_composition_resolves_composing_before_edit() {
        // Regression for #228 review P1: an ordinary edit (backspace) while
        // composing must not leave a stale `composing` range pointing past
        // the now-shorter text, or a later cancel/commit panics slicing it.
        let mut state = TextEditState::new("");
        state.set_composing("x", None);
        assert_eq!(state.text(), "x");
        state.backspace();
        assert_eq!(state.text(), "");
        assert!(!state.is_composing());
        // Previously panicked: `composing` was still `Some((0, 1))` here,
        // one byte past the end of the now-empty string.
        state.cancel_composing();
        assert_eq!(state.text(), "");
    }

    #[test]
    fn insert_during_composition_resolves_composing_before_edit() {
        let mut state = TextEditState::new("ab");
        state.move_left(false);
        state.set_composing("x", None);
        assert_eq!(state.text(), "axb");
        state.insert_char('y');
        assert_eq!(state.text(), "axyb");
        assert!(!state.is_composing());
        // No-op: composing was already resolved by the insert above.
        state.cancel_composing();
        assert_eq!(state.text(), "axyb");
    }

    #[test]
    fn paste_strips_line_breaks_to_stay_single_line() {
        // Regression for #228 review P2: this is a single-line field, so a
        // multi-line paste is flattened onto one line rather than breaking
        // the caret/selection model's one-line assumption.
        let mut state = TextEditState::new("");
        state.paste("first\r\nsecond");
        assert_eq!(state.text(), "firstsecond");
    }

    #[test]
    fn ime_composing_and_commit_strip_line_breaks() {
        let mut state = TextEditState::new("");
        state.set_composing("a\nb", None);
        assert_eq!(state.text(), "ab");
        state.commit_composing("c\r\nd");
        assert_eq!(state.text(), "cd");
    }

    #[test]
    fn default_state_is_empty() {
        let state = TextEditState::default();
        assert_eq!(state.text(), "");
        assert_eq!(state.caret(), 0);
        assert_eq!(state.selection(), None);
        assert!(!state.is_composing());
    }
}
