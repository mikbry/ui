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
//!
//! ## Invariants
//!
//! Two invariants hold after every mutating method returns:
//!
//! 1. The selection anchor is `None` whenever it would coincide with the
//!    caret, or whenever a composition is in progress (composing owns the
//!    caret exclusively while it's active — see `normalize`).
//! 2. The composing range is always valid relative to the *current* `text`
//!    — no method that changes `text`'s content mutates it without first
//!    resolving any stale composing range (see
//!    `resolve_composing_before_edit`).
//!
//! An earlier version of this module maintained these ad hoc, at whichever
//! call sites a review happened to catch; fixing one case (a collapsed
//! selection) left another (an active composition) unguarded, because
//! nothing forced every mutating method to maintain both. Every mutating
//! method now ends by calling `normalize()`, and every method that edits
//! `text` directly (outside the composing lifecycle itself) starts by
//! calling `resolve_composing_before_edit()` — so a new mutating method
//! added later only has to remember "call these," not re-derive the
//! invariants from scratch.

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
    /// New state with `initial` as the text and the caret at its end. Line
    /// breaks are stripped, same as every other way text enters this type
    /// ([`Self::insert_str`], [`Self::set_composing`],
    /// [`Self::commit_composing`]) — this is a single-line field.
    pub fn new(initial: impl Into<String>) -> Self {
        let initial = initial.into();
        let text = strip_line_breaks(&initial).into_owned();
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
        self.normalize();
    }

    /// Move the caret one char left. A plain (non-extending) move with an
    /// active selection collapses to the selection start instead of moving
    /// one further char, matching standard text-field behavior.
    pub fn move_left(&mut self, extend_selection: bool) {
        if !extend_selection {
            if let Some((start, _)) = self.selection() {
                self.anchor = None;
                self.caret = start;
                self.normalize();
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
                self.normalize();
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
        self.normalize();
    }

    pub fn clear_selection(&mut self) {
        self.anchor = None;
        self.normalize();
    }

    /// Delete the current selection, if any, collapsing the caret to its
    /// start. Returns the removed text. Never needs to call
    /// `resolve_composing_before_edit` itself: a selection can't
    /// coexist with an in-progress composition (`normalize`
    /// enforces that), so whenever this actually mutates `text`, composing
    /// is already `None`.
    fn delete_selection(&mut self) -> Option<String> {
        let (start, end) = self.selection()?;
        let removed = self.text[start..end].to_string();
        self.text.replace_range(start..end, "");
        self.caret = start;
        self.anchor = None;
        self.normalize();
        Some(removed)
    }

    /// Insert `s` at the caret, replacing the selection if any. The caret
    /// lands just after the inserted text. Line breaks are stripped — this
    /// is a single-line field, so pasted or typed multi-line text is
    /// flattened onto one line rather than silently violating the
    /// single-line caret/selection rendering contract.
    pub fn insert_str(&mut self, s: &str) {
        let s = strip_line_breaks(s);
        // Only resolve composing when this will actually change `text` —
        // an empty insert with nothing selected is a true no-op and must
        // not have the side effect of silently ending an unrelated
        // in-progress composition.
        if !s.is_empty() || self.selection().is_some() {
            self.resolve_composing_before_edit();
        }
        self.delete_selection();
        self.text.insert_str(self.caret, &s);
        self.caret += s.len();
        self.normalize();
    }

    pub fn insert_char(&mut self, c: char) {
        let mut buf = [0u8; 4];
        self.insert_str(c.encode_utf8(&mut buf));
    }

    /// Delete the selection, or the char before the caret.
    pub fn backspace(&mut self) {
        if self.delete_selection().is_none() && self.caret != 0 {
            self.resolve_composing_before_edit();
            let start = prev_char_boundary(&self.text, self.caret);
            self.text.replace_range(start..self.caret, "");
            self.caret = start;
        }
        self.normalize();
    }

    /// Delete the selection, or the char after the caret.
    pub fn delete(&mut self) {
        if self.delete_selection().is_none() && self.caret != self.text.len() {
            self.resolve_composing_before_edit();
            let end = next_char_boundary(&self.text, self.caret);
            self.text.replace_range(self.caret..end, "");
        }
        self.normalize();
    }

    /// The selected text, without mutating. `None` if there's no selection
    /// — always the case while composing, since a selection can't coexist
    /// with an in-progress composition (`normalize` enforces that). The
    /// host-provided clipboard (#206) stores the result; this never
    /// touches an OS clipboard itself.
    pub fn copy(&self) -> Option<String> {
        let (start, end) = self.selection()?;
        Some(self.text[start..end].to_string())
    }

    /// Remove and return the selected text, for the host clipboard (#206).
    /// Always `None` while composing, for the same reason as [`Self::copy`]
    /// — and since it then never touches `text`, it must never touch
    /// `composing` either (unlike [`Self::insert_str`] / [`Self::backspace`]
    /// / [`Self::delete`], a no-op `cut` is not an edit).
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
        // `cursor` is a byte offset into the *unstripped* `preedit` the host
        // handed us, so it must be clamped and remapped against that string
        // before stripping — remapping against the already-stripped string
        // would leave it pointing at whatever byte happened to end up at
        // that offset post-strip, which isn't a char boundary in general
        // (e.g. `cursor` landing inside a multi-byte char that shifted left
        // to fill the gap left by a removed `\n`).
        let cursor = cursor.unwrap_or(preedit.len()).min(preedit.len());
        let (preedit, cursor) = strip_line_breaks_with_cursor(preedit, cursor);
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
        self.caret = start + cursor;
        self.normalize();
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
        self.normalize();
    }

    /// Cancel an in-progress composition, removing the preedit text
    /// entirely and restoring the caret to where composition started.
    /// No-op if no composition is in progress.
    pub fn cancel_composing(&mut self) {
        if let Some((start, end)) = self.composing.take() {
            self.text.replace_range(start..end, "");
            self.caret = start;
        }
        self.normalize();
    }

    /// Resolve an in-progress IME composition in place — stop tracking its
    /// range, leaving the preedit text already live in `text` as ordinary
    /// committed text — before any edit that changes `text`'s content and
    /// isn't itself part of the composition lifecycle ([`Self::set_composing`]
    /// / [`Self::commit_composing`] / [`Self::cancel_composing`], which
    /// manage `composing` directly). Without this, such an edit can shift
    /// or shrink `text` while a stale `composing` range still points at the
    /// old positions, so a later `commit_composing` / `cancel_composing`
    /// slices out of bounds and panics. [`Self::insert_str`],
    /// [`Self::backspace`], and [`Self::delete`] call this right before any
    /// point where they're about to mutate `text` directly outside the
    /// composition lifecycle itself. ([`Self::cut`] doesn't need to: a
    /// selection can't coexist with an in-progress composition, so its only
    /// text mutation, via [`Self::delete_selection`], never runs while
    /// composing is active.)
    fn resolve_composing_before_edit(&mut self) {
        self.composing = None;
    }

    /// Enforce the invariants every mutating method must leave true — see
    /// the module-level "Invariants" doc. Every mutating method ends by
    /// calling this.
    fn normalize(&mut self) {
        // A selection collapsed back onto its anchor (e.g. shift+right back
        // to where shift+left started) must drop the anchor, not just leave
        // it coincident with the caret — otherwise it resurfaces as a
        // selection, with byte offsets that are no longer valid, once a
        // later edit moves the caret away again.
        if self.anchor == Some(self.caret) {
            self.anchor = None;
        }
        // A selection and an in-progress composition are mutually
        // exclusive: the composition owns the caret position while it's
        // active, so any anchor set while composing (e.g. a shift+arrow or
        // select_all while an IME composition is in progress) is dropped
        // rather than left to coexist with it.
        if self.composing.is_some() {
            self.anchor = None;
        }
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

/// Like [`strip_line_breaks`], but also remaps a byte offset that's valid
/// in `s` (e.g. an IME preedit cursor) into the stripped string — not just
/// clamping it to the new, shorter length, but actually subtracting the
/// bytes removed *before* it, since those bytes no longer exist in the
/// result. Without this, a cursor that lands after a removed `\r`/`\n` ends
/// up pointing at whatever byte shifted into that offset post-strip, which
/// in general isn't even a char boundary (e.g. the middle of a multi-byte
/// char that shifted left to fill the gap) — a cursor in [`TextEditState`]
/// must always land on one.
fn strip_line_breaks_with_cursor(s: &str, cursor: usize) -> (std::borrow::Cow<'_, str>, usize) {
    if !s.contains(['\r', '\n']) {
        return (std::borrow::Cow::Borrowed(s), cursor);
    }
    let removed_before_cursor: usize = s
        .char_indices()
        .take_while(|(byte_idx, _)| *byte_idx < cursor)
        .filter(|(_, c)| *c == '\r' || *c == '\n')
        .map(|(_, c)| c.len_utf8())
        .sum();
    let stripped = s.chars().filter(|c| *c != '\r' && *c != '\n').collect();
    (
        std::borrow::Cow::Owned(stripped),
        cursor - removed_before_cursor,
    )
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
    fn new_strips_line_breaks_to_stay_single_line() {
        // Regression for #228 review round 2 P2: `new()` is also a way text
        // enters this type, so it must apply the same single-line
        // normalization as insert_str/set_composing/commit_composing.
        let state = TextEditState::new("first\r\nsecond");
        assert_eq!(state.text(), "firstsecond");
        assert_eq!(state.caret(), "firstsecond".len());
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
    fn set_composing_cursor_remap_does_not_split_a_multibyte_char() {
        // Regression for #228 review round 2 P1: `cursor` is a byte offset
        // into the *unstripped* preedit the host supplied. "\néx" with
        // cursor 1 means "right after the \n, right before é" — once the
        // \n is stripped, that must land at byte 0 of "éx" (before é, a
        // valid char boundary), not byte 1 (which would be the middle of
        // é's 2-byte UTF-8 encoding).
        let mut state = TextEditState::new("");
        state.set_composing("\néx", Some(1));
        assert_eq!(state.text(), "éx");
        assert_eq!(
            state.caret(),
            0,
            "cursor must land before é, not mid-byte inside it"
        );
        // Previously, an unremapped cursor landed at byte 1 — inside é's
        // 2-byte encoding — and this would panic slicing a non-boundary.
        state.delete();
        assert_eq!(
            state.text(),
            "x",
            "delete must remove all of é, not split it"
        );
    }

    #[test]
    fn set_composing_cursor_remap_counts_only_breaks_before_the_cursor() {
        // A line break *after* the cursor must not affect the remap.
        let mut state = TextEditState::new("");
        state.set_composing("ab\ncd", Some(1));
        assert_eq!(state.text(), "abcd");
        assert_eq!(state.caret(), 1);
    }

    #[test]
    fn set_composing_cursor_remap_handles_crlf_before_cursor() {
        let mut state = TextEditState::new("");
        state.set_composing("ab\r\ncd", Some(5));
        assert_eq!(state.text(), "abcd");
        // Both \r and \n (2 bytes) were before the original cursor (5), so
        // the remapped cursor is 5 - 2 = 3, landing right before 'd'.
        assert_eq!(state.caret(), 3);
    }

    // --- Structural audit regressions (#228 review follow-up): fixing the
    // selection/composing interaction above one call site at a time missed
    // others. These exercise every remaining text/caret/selection-mutating
    // method against both invariants (anchor collapses when it would
    // coincide with the caret or while composing; composing never outlives
    // a text edit that doesn't itself resolve it).

    #[test]
    fn delete_during_composition_resolves_composing_before_edit() {
        let mut state = TextEditState::new("");
        state.set_composing("x", None);
        state.move_home(false); // caret before the preedit, so delete() hits it
        state.delete();
        assert_eq!(state.text(), "");
        assert!(!state.is_composing());
        // Would previously panic: a stale `composing` range past the end.
        state.cancel_composing();
        assert_eq!(state.text(), "");
    }

    #[test]
    fn paste_during_composition_resolves_composing_before_edit() {
        let mut state = TextEditState::new("");
        state.set_composing("xx", None);
        state.paste("y");
        // The preedit "xx" is already live in `text`; resolving composing
        // in place leaves it as ordinary text, so "y" lands after it.
        assert_eq!(state.text(), "xxy");
        assert!(!state.is_composing());
        // Would previously panic: a stale `composing` range past content
        // that's since shifted/grown.
        state.cancel_composing();
        assert_eq!(state.text(), "xxy");
    }

    #[test]
    fn empty_paste_during_composition_does_not_disturb_composing() {
        // Unlike a real insert, an empty insert changes nothing — it must
        // not have the side effect of silently ending an unrelated
        // in-progress composition.
        let mut state = TextEditState::new("");
        state.set_composing("x", None);
        state.paste("");
        assert_eq!(state.text(), "x");
        assert!(state.is_composing(), "empty paste must not end composing");
    }

    #[test]
    fn backspace_at_start_during_composition_does_not_disturb_composing() {
        // A backspace that deletes nothing (caret at the very start, no
        // selection) must not have the side effect of silently ending an
        // unrelated in-progress composition.
        let mut state = TextEditState::new("");
        state.set_composing("x", None);
        state.move_home(false);
        state.backspace();
        assert_eq!(state.text(), "x");
        assert!(
            state.is_composing(),
            "no-op backspace must not end composing"
        );
    }

    #[test]
    fn delete_at_end_during_composition_does_not_disturb_composing() {
        let mut state = TextEditState::new("");
        state.set_composing("x", None);
        state.move_end(false);
        state.delete();
        assert_eq!(state.text(), "x");
        assert!(state.is_composing(), "no-op delete must not end composing");
    }

    #[test]
    fn select_all_while_composing_does_not_create_a_selection() {
        // A selection and an in-progress composition are mutually
        // exclusive — the composition owns the caret while it's active.
        let mut state = TextEditState::new("ab");
        state.move_left(false);
        state.set_composing("x", None);
        state.select_all();
        assert_eq!(
            state.selection(),
            None,
            "select_all must not create a selection during composition"
        );
        assert!(state.is_composing(), "composing must survive select_all");
    }

    #[test]
    fn shift_arrow_while_composing_does_not_create_a_selection() {
        let mut state = TextEditState::new("ab");
        state.move_left(false);
        state.set_composing("x", None);
        state.move_left(true);
        assert_eq!(
            state.selection(),
            None,
            "shift+arrow must not create a selection during composition"
        );
    }

    #[test]
    fn cut_and_copy_while_composing_return_none_and_leave_composing_untouched() {
        // No selection can exist while composing (enforced by `normalize`),
        // so cut/copy during composition always have nothing to act on —
        // and, unlike insert_str/backspace/delete, a no-op cut is not an
        // edit, so it must not end the composition as a side effect.
        let mut state = TextEditState::new("ab");
        state.move_left(false);
        state.set_composing("x", None);
        state.select_all(); // still no selection — composing excludes it
        assert_eq!(state.copy(), None);
        assert_eq!(state.cut(), None);
        assert_eq!(
            state.text(),
            "axb",
            "cut must not touch text with nothing to cut"
        );
        assert!(
            state.is_composing(),
            "cut/copy must not disturb an unrelated composition"
        );
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
