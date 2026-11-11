//! Console-side projection of an [`mkui_runtime::AppTree`].
//!
//! Sprint 4: the console backend now walks the shared runtime tree instead
//! of the old `Vec<Box<dyn Component>>` shape. Layout is still a flat list
//! of [`Line`]s plus the [`Selectable`] records (one per interactive
//! control — `Button` or `MenuList` item) that [`crate::high_level::Mkui`]
//! navigates and renders.
//!
//! Styling decisions come from the *typed* [`TextVariant`] /
//! [`ButtonVariant`] values on each node — never from sniffing
//! showcase-specific class strings.
//!
//! ## One cursor across every control (#189 round 3)
//!
//! `Button` and `MenuList` rows used to be tracked separately — buttons in
//! their own `Vec`, menu items carrying a baked-in `selected` flag decided
//! at walk time. That meant a tree with only a `MenuList` had nothing for
//! the run loop to navigate (it gates on "any buttons?"), and a tree mixing
//! both only ever moved the button cursor. [`Selectable`] unifies both
//! kinds into one `Vec` in tree-walk order; [`crate::high_level::Mkui`]
//! keeps a single live cursor over it, so Up/Down/Enter work the same way
//! regardless of which kind of control is currently selected, and "is this
//! row selected" is decided once, at render time, from that one cursor —
//! never baked into a `Line` at walk time.

use mkui_runtime::{ActionId, AppTree, ButtonVariant, NodeKind, TextVariant};

/// Console-side projection of a runtime `Button` node.
///
/// Holds the label, the variant the styling needs, and the action id so
/// the navigation loop can fire the registered closure via the tree's
/// `ActionRegistry`.
#[derive(Clone, Debug)]
pub struct ConsoleButton {
    pub label: String,
    pub variant: ButtonVariant,
    pub on_press: Option<ActionId>,
}

/// Console-side projection of one row of a `"menu_list"` `NodeKind::Custom`
/// node (written by `mkui_core::components::MenuList::lower_to_tree`).
#[derive(Clone, Debug)]
pub struct ConsoleMenuItem {
    pub label: String,
    /// Action fired on confirm, if the `MenuList` was built with
    /// `.on_select(...)`. One action per item, each closing over its own
    /// fixed index — see `MenuList::on_select`'s doc comment.
    pub on_select: Option<ActionId>,
    /// Whether this item was the `MenuList`'s *construction-time* default
    /// selection (`.selected_index(...)`, or item 0 otherwise). The run
    /// loop reads this exactly once, to seed its live cursor before the
    /// first render — it is not consulted again afterwards.
    pub initial: bool,
}

/// One interactive control, in tree-walk order. [`crate::high_level::Mkui`]
/// keeps a single selection cursor as an index into a `Vec<Selectable>`
/// built by [`walk_tree`], so Up/Down/Enter behave identically whether the
/// row under the cursor is a `Button` or a `MenuList` item.
#[derive(Clone, Debug)]
pub enum Selectable {
    Button(ConsoleButton),
    MenuItem(ConsoleMenuItem),
}

impl Selectable {
    /// The action to fire when this control is confirmed (Enter/Space),
    /// if any was registered.
    pub fn on_activate(&self) -> Option<ActionId> {
        match self {
            Selectable::Button(button) => button.on_press,
            Selectable::MenuItem(item) => item.on_select,
        }
    }

    pub fn label(&self) -> &str {
        match self {
            Selectable::Button(button) => &button.label,
            Selectable::MenuItem(item) => &item.label,
        }
    }
}

/// One row in the flattened render plan produced from a runtime tree.
///
/// The console backend can't paint nested boxes, so it flattens the tree
/// into a sequence of lines. [`Line::Button`] and [`Line::MenuListItem`]
/// both reference the matching [`Selectable`] in the parent app's unified
/// cursor space by index — "is this row selected" is a render-time
/// comparison against that one live cursor, not data carried on the `Line`
/// itself (#189 round 3: a `Line`-baked `selected` flag can't reflect
/// input that happens after the tree was walked).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Line {
    Heading(String),
    Body(String),
    Muted(String),
    Spacer,
    Button(usize),
    MenuListItem(usize),
}

/// Walk an [`AppTree`] and flatten it into lines + a unified list of
/// interactive controls for the terminal renderer.
pub fn walk_tree(tree: &AppTree, layout: &mut Vec<Line>, selectables: &mut Vec<Selectable>) {
    let root = match tree.get(tree.root()) {
        Some(node) => node,
        None => return,
    };
    walk_children(tree, &root.children, layout, selectables);
}

fn walk_children(
    tree: &AppTree,
    children: &[mkui_runtime::NodeId],
    layout: &mut Vec<Line>,
    selectables: &mut Vec<Selectable>,
) {
    for child_id in children {
        let Some(node) = tree.get(*child_id) else {
            continue;
        };
        match &node.kind {
            NodeKind::View(_) => {
                walk_children(tree, &node.children, layout, selectables);
            }
            NodeKind::Text(t) => {
                let content = t.content.clone();
                let line = match t.variant {
                    TextVariant::Heading1 | TextVariant::Heading2 | TextVariant::Heading3 => {
                        Line::Heading(content)
                    }
                    TextVariant::Caption | TextVariant::Label => Line::Muted(content),
                    TextVariant::Body | TextVariant::Code => Line::Body(content),
                    _ => Line::Body(content),
                };
                layout.push(line);
                layout.push(Line::Spacer);
            }
            NodeKind::Button(b) => {
                let index = selectables.len();
                selectables.push(Selectable::Button(ConsoleButton {
                    label: b.label.clone(),
                    variant: b.variant,
                    on_press: b.on_press,
                }));
                layout.push(Line::Button(index));
            }
            NodeKind::Custom { type_name, props } if type_name == "menu_list" => {
                push_menu_list_lines(props, layout, selectables);
            }
            NodeKind::Root | NodeKind::Custom { .. } => {
                // Root is unreachable here (we started at its children). An
                // unrecognized custom node has no built-in console
                // rendering — Sprint 6+ extension registry will define a
                // general dispatch (`"menu_list"` above is the first
                // special case, added ad hoc rather than waiting on that
                // registry). For now we recurse into any children so a
                // custom container still surfaces its descendents.
                walk_children(tree, &node.children, layout, selectables);
            }
        }
    }
}

/// Decode a `"menu_list"` [`NodeKind::Custom`] node's `props` (written by
/// `mkui_core::components::MenuList::lower_to_tree`) into one
/// [`ConsoleMenuItem`]/[`Line::MenuListItem`] pair per item.
fn push_menu_list_lines(
    props: &serde_json::Value,
    layout: &mut Vec<Line>,
    selectables: &mut Vec<Selectable>,
) {
    let selected_index = props.get("selected_index").and_then(|v| v.as_u64());
    let items = props
        .get("items")
        .and_then(|v| v.as_array())
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let item_actions = props
        .get("item_actions")
        .and_then(|v| v.as_array())
        .map(Vec::as_slice)
        .unwrap_or(&[]);

    for (local_index, item) in items.iter().enumerate() {
        let label = item.as_str().unwrap_or_default().to_string();
        let on_select = item_actions
            .get(local_index)
            .and_then(|v| serde_json::from_value::<ActionId>(v.clone()).ok());
        let initial = selected_index == Some(local_index as u64);

        let global_index = selectables.len();
        selectables.push(Selectable::MenuItem(ConsoleMenuItem {
            label,
            on_select,
            initial,
        }));
        layout.push(Line::MenuListItem(global_index));
    }
    layout.push(Line::Spacer);
}

#[cfg(test)]
mod tests {
    use super::*;
    use mkui_core::components::{Button, MenuList, Mkui, Text, View};
    use mkui_core::headless::TextVariant;
    use std::cell::Cell;
    use std::rc::Rc;

    #[test]
    fn walk_tree_preserves_on_press_action() {
        let pressed = Rc::new(Cell::new(0u32));
        let pressed_in = Rc::clone(&pressed);

        let app = Mkui::new().child(Button::new("ok").on_press(move || {
            pressed_in.set(pressed_in.get() + 1);
        }));

        let mut layout = Vec::new();
        let mut selectables = Vec::new();
        walk_tree(app.tree(), &mut layout, &mut selectables);

        assert_eq!(selectables.len(), 1);
        let action = selectables[0]
            .on_activate()
            .expect("on_press must be registered");
        app.tree().actions().fire(action);
        app.tree().actions().fire(action);
        assert_eq!(pressed.get(), 2);
    }

    #[test]
    fn walk_tree_recurses_into_nested_views() {
        let pressed = Rc::new(Cell::new(false));
        let pressed_in = Rc::clone(&pressed);

        let app = Mkui::new().child(View::new().child(View::new().child(
            Button::new("deep").on_press(move || {
                pressed_in.set(true);
            }),
        )));

        let mut layout = Vec::new();
        let mut selectables = Vec::new();
        walk_tree(app.tree(), &mut layout, &mut selectables);

        assert_eq!(selectables.len(), 1);
        let action = selectables[0].on_activate().expect("on_press");
        app.tree().actions().fire(action);
        assert!(pressed.get());
    }

    #[test]
    fn text_variant_drives_line_style() {
        let app = Mkui::new().child(
            View::new()
                .child(
                    Text::new("title")
                        .variant(TextVariant::Heading1)
                        .class("text-4xl"),
                )
                .child(
                    Text::new("note")
                        .variant(TextVariant::Caption)
                        .class("text-sm"),
                )
                .child(Text::new("body")),
        );

        let mut layout = Vec::new();
        let mut selectables = Vec::new();
        walk_tree(app.tree(), &mut layout, &mut selectables);

        let lines: Vec<&Line> = layout
            .iter()
            .filter(|l| !matches!(l, Line::Spacer))
            .collect();
        assert_eq!(lines.len(), 3);
        assert_eq!(*lines[0], Line::Heading("title".into()));
        assert_eq!(*lines[1], Line::Muted("note".into()));
        assert_eq!(*lines[2], Line::Body("body".into()));
    }

    /// Structural half of issue #189 acceptance criterion #2: every
    /// `MenuList` item must produce a `Line::MenuListItem` referencing a
    /// `Selectable::MenuItem` with the right label, and exactly the item
    /// matching the `MenuList`'s construction-time `selected_index` must
    /// come back with `initial: true` — that's the only thing `walk_tree`
    /// itself decides. Whether a row renders as *currently* selected is a
    /// render-time comparison against the live cursor
    /// (`high_level::Mkui`'s own test, `menu_list_input_moves_the_rendered_
    /// highlight_and_confirms`, covers that end to end).
    #[test]
    fn menu_list_walk_produces_one_selectable_per_item_with_the_right_initial_flag() {
        let app = Mkui::new().child(
            MenuList::new()
                .item("New Game")
                .item("Continue")
                .item("Quit")
                .selected_index(1),
        );

        let mut layout = Vec::new();
        let mut selectables = Vec::new();
        walk_tree(app.tree(), &mut layout, &mut selectables);

        let item_lines: Vec<usize> = layout
            .iter()
            .filter_map(|l| match l {
                Line::MenuListItem(index) => Some(*index),
                _ => None,
            })
            .collect();
        assert_eq!(item_lines, vec![0, 1, 2], "one Line per item, in order");

        let menu_items: Vec<&ConsoleMenuItem> = selectables
            .iter()
            .map(|s| match s {
                Selectable::MenuItem(item) => item,
                Selectable::Button(_) => panic!("expected only menu items"),
            })
            .collect();
        assert_eq!(menu_items[0].label, "New Game");
        assert_eq!(menu_items[1].label, "Continue");
        assert_eq!(menu_items[2].label, "Quit");

        assert!(!menu_items[0].initial);
        assert!(
            menu_items[1].initial,
            "selected_index(1) must mark exactly item 1 as the initial selection"
        );
        assert!(!menu_items[2].initial);
    }

    #[test]
    fn menu_list_defaults_initial_to_item_zero() {
        let app = Mkui::new().child(MenuList::new().item("A").item("B").item("C"));

        let mut layout = Vec::new();
        let mut selectables = Vec::new();
        walk_tree(app.tree(), &mut layout, &mut selectables);

        let initial_flags: Vec<bool> = selectables
            .iter()
            .map(|s| match s {
                Selectable::MenuItem(item) => item.initial,
                Selectable::Button(_) => panic!("expected only menu items"),
            })
            .collect();
        assert_eq!(initial_flags, vec![true, false, false]);
    }

    #[test]
    fn menu_list_item_actions_round_trip_through_walk_tree() {
        let fired = Rc::new(Cell::new(None));
        let fired_in = Rc::clone(&fired);

        let app = Mkui::new().child(
            MenuList::new()
                .item("New Game")
                .item("Quit")
                .on_select(move |index| fired_in.set(Some(index))),
        );

        let mut layout = Vec::new();
        let mut selectables = Vec::new();
        walk_tree(app.tree(), &mut layout, &mut selectables);

        let quit_action = selectables[1]
            .on_activate()
            .expect("on_select must produce an action for every item");
        app.tree().actions().fire(quit_action);
        assert_eq!(fired.get(), Some(1));
    }
}
