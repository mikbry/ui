use super::traits::{Focusable, HeadlessComponent, KeyboardInteractable};
use crate::event::Event;
use crate::state::State;

/// A single entry in a [`MenuList`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuListItem {
    label: String,
}

impl MenuListItem {
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
        }
    }

    pub fn label(&self) -> &str {
        &self.label
    }
}

/// State for a menu list component
#[derive(Debug, Clone, Default)]
pub struct MenuListState {
    pub focused: bool,
    pub selected_index: Option<usize>,
}

impl State for MenuListState {}

/// Events for menu list component
#[derive(Debug, Clone)]
pub enum MenuListEvent {
    Up,
    Down,
    Confirm,
    Focus,
    Blur,
    KeyDown(String),
    KeyUp(String),
}

impl Event for MenuListEvent {}

/// Headless menu list — a title-screen/pause-style list of items with a
/// single current selection, navigated with up/down and activated with a
/// confirm action (issue #189, C4 of #173).
///
/// ## Keyboard *and* gamepad from one input path
///
/// `MenuList` has no gamepad-specific code. Per the backend-agnostic input
/// contract in [`crate::input`], a host maps its native input — keyboard
/// arrow keys, but equally a gamepad D-pad/stick and confirm button — into
/// the shared [`crate::input::Key`] vocabulary before dispatch. `MenuList`
/// only ever sees the result of that mapping through
/// [`KeyboardInteractable::handle_key_down`], so a gamepad "down" and a
/// keyboard `ArrowDown` are indistinguishable by the time they reach here —
/// which is exactly the point: one navigation path, not two.
///
/// ## Composing with a focus trap
///
/// `MenuList` only reacts to `ArrowUp`/`ArrowDown`/`Enter`/`" "` — it never
/// looks at `Tab` or `Escape`. A host embedding this inside a modal dialog's
/// focus trap (#188) is free to own those keys for trap-cycling and dismiss;
/// `MenuList` just moves its own selection when the trap hands it focus and
/// forwards a key, and otherwise stays out of the way.
pub struct MenuList {
    state: MenuListState,
    items: Vec<MenuListItem>,
    on_select: Option<Box<dyn Fn(usize)>>,
}

impl MenuList {
    pub fn builder() -> MenuListBuilder {
        MenuListBuilder::new()
    }

    pub fn items(&self) -> &[MenuListItem] {
        &self.items
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn selected_index(&self) -> Option<usize> {
        self.state.selected_index
    }

    pub fn selected_item(&self) -> Option<&MenuListItem> {
        self.state
            .selected_index
            .and_then(|index| self.items.get(index))
    }

    /// Whether `index` is the current selection. Renderers use this to give
    /// the selected row a visually distinct style (acceptance criterion #2).
    pub fn is_selected(&self, index: usize) -> bool {
        self.state.selected_index == Some(index)
    }

    pub fn select_index(&mut self, index: usize) {
        if index < self.items.len() {
            self.state.selected_index = Some(index);
        }
    }

    /// Move the selection to the previous item, wrapping from the first
    /// item to the last. No-op on an empty list.
    pub fn move_up(&mut self) {
        if self.items.is_empty() {
            return;
        }
        let next = match self.state.selected_index {
            Some(0) | None => self.items.len() - 1,
            Some(index) => index - 1,
        };
        self.state.selected_index = Some(next);
    }

    /// Move the selection to the next item, wrapping from the last item to
    /// the first. No-op on an empty list.
    pub fn move_down(&mut self) {
        if self.items.is_empty() {
            return;
        }
        let next = match self.state.selected_index {
            Some(index) if index + 1 < self.items.len() => index + 1,
            _ => 0,
        };
        self.state.selected_index = Some(next);
    }

    /// Activate the current selection, firing `on_select` with its index.
    /// No-op when nothing is selected (e.g. an empty list).
    pub fn confirm(&mut self) {
        if let Some(index) = self.state.selected_index {
            if let Some(on_select) = &self.on_select {
                on_select(index);
            }
        }
    }
}

impl HeadlessComponent for MenuList {
    type State = MenuListState;
    type Event = MenuListEvent;

    fn new() -> Self {
        Self {
            state: MenuListState::default(),
            items: Vec::new(),
            on_select: None,
        }
    }

    fn state(&self) -> &Self::State {
        &self.state
    }

    fn handle_event(&mut self, event: Self::Event) {
        match event {
            MenuListEvent::Up => self.move_up(),
            MenuListEvent::Down => self.move_down(),
            MenuListEvent::Confirm => self.confirm(),
            MenuListEvent::Focus => self.focus(),
            MenuListEvent::Blur => self.blur(),
            MenuListEvent::KeyDown(key) => self.handle_key_down(&key),
            MenuListEvent::KeyUp(key) => self.handle_key_up(&key),
        }
    }
}

impl Focusable for MenuList {
    fn focus(&mut self) {
        self.state.focused = true;
    }

    fn blur(&mut self) {
        self.state.focused = false;
    }

    fn is_focused(&self) -> bool {
        self.state.focused
    }
}

impl KeyboardInteractable for MenuList {
    fn handle_key_down(&mut self, key: &str) {
        match key {
            "ArrowUp" => self.move_up(),
            "ArrowDown" => self.move_down(),
            "Enter" | " " => self.confirm(),
            _ => {}
        }
    }

    fn handle_key_up(&mut self, _key: &str) {
        // Navigation and confirm both act on key-down; nothing to do here.
    }
}

/// Builder for MenuList component
pub struct MenuListBuilder {
    items: Vec<MenuListItem>,
    on_select: Option<Box<dyn Fn(usize)>>,
}

impl MenuListBuilder {
    pub fn new() -> Self {
        Self {
            items: Vec::new(),
            on_select: None,
        }
    }

    pub fn item(mut self, label: impl Into<String>) -> Self {
        self.items.push(MenuListItem::new(label));
        self
    }

    pub fn items<I, S>(mut self, labels: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.items.extend(labels.into_iter().map(MenuListItem::new));
        self
    }

    pub fn on_select<F: Fn(usize) + 'static>(mut self, f: F) -> Self {
        self.on_select = Some(Box::new(f));
        self
    }

    /// Builds the list with the first item pre-selected (when non-empty),
    /// so a freshly-built menu always has a visually distinct selection
    /// rather than requiring a first keypress to establish one.
    pub fn build(self) -> MenuList {
        let selected_index = if self.items.is_empty() { None } else { Some(0) };
        MenuList {
            state: MenuListState {
                focused: false,
                selected_index,
            },
            items: self.items,
            on_select: self.on_select,
        }
    }
}

impl Default for MenuListBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    #[test]
    fn build_preselects_first_item() {
        let menu = MenuList::builder()
            .item("New Game")
            .item("Continue")
            .build();
        assert_eq!(menu.selected_index(), Some(0));
        assert!(menu.is_selected(0));
        assert!(!menu.is_selected(1));
    }

    #[test]
    fn empty_menu_has_no_selection_and_ignores_navigation() {
        let mut menu = MenuList::builder().build();
        assert_eq!(menu.selected_index(), None);
        menu.move_down();
        menu.move_up();
        assert_eq!(menu.selected_index(), None);
    }

    #[test]
    fn down_moves_forward_and_wraps() {
        let mut menu = MenuList::builder()
            .item("New Game")
            .item("Continue")
            .item("Quit")
            .build();

        assert_eq!(menu.selected_index(), Some(0));
        menu.move_down();
        assert_eq!(menu.selected_index(), Some(1));
        menu.move_down();
        assert_eq!(menu.selected_index(), Some(2));
        menu.move_down();
        assert_eq!(
            menu.selected_index(),
            Some(0),
            "must wrap to the first item"
        );
    }

    #[test]
    fn up_moves_backward_and_wraps() {
        let mut menu = MenuList::builder()
            .item("New Game")
            .item("Continue")
            .item("Quit")
            .build();

        assert_eq!(menu.selected_index(), Some(0));
        menu.move_up();
        assert_eq!(menu.selected_index(), Some(2), "must wrap to the last item");
        menu.move_up();
        assert_eq!(menu.selected_index(), Some(1));
    }

    #[test]
    fn confirm_fires_on_select_with_the_selected_index() {
        let fired = Rc::new(RefCell::new(None));
        let fired_in = Rc::clone(&fired);

        let mut menu = MenuList::builder()
            .item("New Game")
            .item("Continue")
            .on_select(move |index| *fired_in.borrow_mut() = Some(index))
            .build();

        menu.move_down();
        menu.confirm();
        assert_eq!(*fired.borrow(), Some(1));
    }

    #[test]
    fn confirm_on_empty_menu_does_not_fire() {
        let fired = Rc::new(RefCell::new(false));
        let fired_in = Rc::clone(&fired);

        let mut menu = MenuList::builder()
            .on_select(move |_| *fired_in.borrow_mut() = true)
            .build();

        menu.confirm();
        assert!(!*fired.borrow());
    }

    #[test]
    fn keyboard_and_gamepad_share_the_same_navigation_path() {
        // A host maps both keyboard arrows and a gamepad D-pad/stick into
        // the same `Key::as_str()` vocabulary before calling
        // `handle_key_down` — so driving it with those literal strings
        // exercises both input sources through one code path.
        let mut menu = MenuList::builder().item("A").item("B").item("C").build();

        menu.handle_key_down("ArrowDown");
        assert_eq!(menu.selected_index(), Some(1));

        menu.handle_key_down("ArrowUp");
        assert_eq!(menu.selected_index(), Some(0));
    }

    #[test]
    fn enter_and_space_both_confirm() {
        let fired = Rc::new(RefCell::new(0u32));

        for key in ["Enter", " "] {
            let fired_in = Rc::clone(&fired);
            let mut menu = MenuList::builder()
                .item("ok")
                .on_select(move |_| *fired_in.borrow_mut() += 1)
                .build();
            menu.handle_key_down(key);
        }

        assert_eq!(*fired.borrow(), 2);
    }

    #[test]
    fn unrelated_keys_are_ignored() {
        let mut menu = MenuList::builder().item("A").item("B").build();
        menu.handle_key_down("Tab");
        menu.handle_key_down("Escape");
        assert_eq!(
            menu.selected_index(),
            Some(0),
            "MenuList must not react to Tab/Escape — those belong to a \
             surrounding focus trap (#188), not this component"
        );
    }

    #[test]
    fn select_index_ignores_out_of_range() {
        let mut menu = MenuList::builder().item("A").item("B").build();
        menu.select_index(5);
        assert_eq!(menu.selected_index(), Some(0));
        menu.select_index(1);
        assert_eq!(menu.selected_index(), Some(1));
    }

    #[test]
    fn focus_lifecycle_matches_other_headless_components() {
        let mut menu = MenuList::builder().item("A").build();
        assert!(!menu.is_focused());
        menu.focus();
        assert!(menu.is_focused());
        menu.blur();
        assert!(!menu.is_focused());
    }

    #[test]
    fn handle_event_dispatches_every_variant() {
        let fired = Rc::new(RefCell::new(None));
        let fired_in = Rc::clone(&fired);
        let mut menu = MenuList::builder()
            .item("A")
            .item("B")
            .on_select(move |index| *fired_in.borrow_mut() = Some(index))
            .build();

        menu.handle_event(MenuListEvent::Focus);
        assert!(menu.is_focused());

        menu.handle_event(MenuListEvent::Down);
        assert_eq!(menu.selected_index(), Some(1));

        menu.handle_event(MenuListEvent::Confirm);
        assert_eq!(*fired.borrow(), Some(1));

        menu.handle_event(MenuListEvent::KeyDown("ArrowUp".to_string()));
        assert_eq!(menu.selected_index(), Some(0));

        menu.handle_event(MenuListEvent::Blur);
        assert!(!menu.is_focused());
    }

    #[test]
    fn items_builder_bulk_adds_labels() {
        let menu = MenuList::builder()
            .items(["New Game", "Continue", "Quit"])
            .build();
        assert_eq!(menu.len(), 3);
        assert_eq!(menu.items()[0].label(), "New Game");
        assert_eq!(menu.items()[2].label(), "Quit");
    }
}
