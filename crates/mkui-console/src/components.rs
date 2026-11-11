//! Console-side projection of an [`mkui_runtime::AppTree`].
//!
//! Sprint 4: the console backend now walks the shared runtime tree instead
//! of the old `Vec<Box<dyn Component>>` shape. Layout is still a flat list
//! of [`Line`]s plus the [`ConsoleButton`] records (label, variant, action
//! id) that [`crate::high_level::Mkui`] navigates and renders.
//!
//! Styling decisions come from the *typed* [`TextVariant`] /
//! [`ButtonVariant`] values on each node — never from sniffing
//! showcase-specific class strings.

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

/// One row in the flattened render plan produced from a runtime tree.
///
/// The console backend can't paint nested boxes, so it flattens the tree
/// into a sequence of lines. [`Line::Button`] references the matching
/// [`ConsoleButton`] in the parent app by index. [`Line::MenuListItem`]
/// carries its own label/selected flag directly rather than indexing a
/// side table, since a `"menu_list"` [`NodeKind::Custom`] node has no
/// action id or variant for a parallel record to track (#189).
#[derive(Clone, Debug, PartialEq)]
pub enum Line {
    Heading(String),
    Body(String),
    Muted(String),
    Spacer,
    Button(usize),
    MenuListItem { label: String, selected: bool },
}

/// Walk an [`AppTree`] and flatten it into lines + buttons for the
/// terminal renderer.
pub fn walk_tree(tree: &AppTree, layout: &mut Vec<Line>, buttons: &mut Vec<ConsoleButton>) {
    let root = match tree.get(tree.root()) {
        Some(node) => node,
        None => return,
    };
    walk_children(tree, &root.children, layout, buttons);
}

fn walk_children(
    tree: &AppTree,
    children: &[mkui_runtime::NodeId],
    layout: &mut Vec<Line>,
    buttons: &mut Vec<ConsoleButton>,
) {
    for child_id in children {
        let Some(node) = tree.get(*child_id) else {
            continue;
        };
        match &node.kind {
            NodeKind::View(_) => {
                walk_children(tree, &node.children, layout, buttons);
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
                let index = buttons.len();
                buttons.push(ConsoleButton {
                    label: b.label.clone(),
                    variant: b.variant,
                    on_press: b.on_press,
                });
                layout.push(Line::Button(index));
            }
            NodeKind::Custom { type_name, props } if type_name == "menu_list" => {
                push_menu_list_lines(props, layout);
            }
            NodeKind::Root | NodeKind::Custom { .. } => {
                // Root is unreachable here (we started at its children). An
                // unrecognized custom node has no built-in console
                // rendering — Sprint 6+ extension registry will define a
                // general dispatch (`"menu_list"` above is the first
                // special case, added ad hoc rather than waiting on that
                // registry). For now we recurse into any children so a
                // custom container still surfaces its descendents.
                walk_children(tree, &node.children, layout, buttons);
            }
        }
    }
}

/// Decode a `"menu_list"` [`NodeKind::Custom`] node's `props` (written by
/// [`mkui_core::components::MenuList::lower_to_tree`]) into one
/// [`Line::MenuListItem`] per item, each carrying whether it is the current
/// selection — the data the renderer needs to make that row visually
/// distinct (issue #189 acceptance criterion #2).
fn push_menu_list_lines(props: &serde_json::Value, layout: &mut Vec<Line>) {
    let selected_index = props.get("selected_index").and_then(|v| v.as_u64());
    let items = props
        .get("items")
        .and_then(|v| v.as_array())
        .map(Vec::as_slice)
        .unwrap_or(&[]);

    for (index, item) in items.iter().enumerate() {
        let label = item.as_str().unwrap_or_default().to_string();
        let selected = selected_index == Some(index as u64);
        layout.push(Line::MenuListItem { label, selected });
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
        let mut buttons = Vec::new();
        walk_tree(app.tree(), &mut layout, &mut buttons);

        assert_eq!(buttons.len(), 1);
        let action = buttons[0].on_press.expect("on_press must be registered");
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
        let mut buttons = Vec::new();
        walk_tree(app.tree(), &mut layout, &mut buttons);

        assert_eq!(buttons.len(), 1);
        let action = buttons[0].on_press.expect("on_press");
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
        let mut buttons = Vec::new();
        walk_tree(app.tree(), &mut layout, &mut buttons);

        let lines: Vec<&Line> = layout
            .iter()
            .filter(|l| !matches!(l, Line::Spacer))
            .collect();
        assert_eq!(lines.len(), 3);
        assert_eq!(*lines[0], Line::Heading("title".into()));
        assert_eq!(*lines[1], Line::Muted("note".into()));
        assert_eq!(*lines[2], Line::Body("body".into()));
    }

    /// Issue #189 acceptance criterion #2 ("current selection is visually
    /// distinct"): walking a `MenuList` node must mark exactly the selected
    /// row's [`Line::MenuListItem`] as `selected`, and the console renderer
    /// (`high_level::Mkui::render`) styles that flag differently from an
    /// unselected row. This test is the backend-facing half of that
    /// guarantee — it proves the indicator survives from the cross-platform
    /// `MenuList` component through to the console-specific render plan.
    #[test]
    fn menu_list_walk_marks_only_the_selected_row() {
        let app = Mkui::new().child(
            MenuList::new()
                .item("New Game")
                .item("Continue")
                .item("Quit")
                .selected_index(1),
        );

        let mut layout = Vec::new();
        let mut buttons = Vec::new();
        walk_tree(app.tree(), &mut layout, &mut buttons);

        let items: Vec<&Line> = layout
            .iter()
            .filter(|l| matches!(l, Line::MenuListItem { .. }))
            .collect();
        assert_eq!(items.len(), 3, "every menu item must produce a line");

        assert_eq!(
            *items[0],
            Line::MenuListItem {
                label: "New Game".into(),
                selected: false
            }
        );
        assert_eq!(
            *items[1],
            Line::MenuListItem {
                label: "Continue".into(),
                selected: true
            },
            "the selection indicator must land on the selected item, not just exist somewhere"
        );
        assert_eq!(
            *items[2],
            Line::MenuListItem {
                label: "Quit".into(),
                selected: false
            }
        );
    }

    /// The indicator must move with the selection, not render a fixed row —
    /// re-walking with a different `selected_index` moves which `Line` has
    /// `selected: true`.
    #[test]
    fn menu_list_selection_indicator_tracks_selected_index() {
        let at_zero = Mkui::new().child(MenuList::new().item("A").item("B").item("C"));
        let mut layout_zero = Vec::new();
        let mut buttons_zero = Vec::new();
        walk_tree(at_zero.tree(), &mut layout_zero, &mut buttons_zero);
        let selected_in_zero: Vec<bool> = layout_zero
            .iter()
            .filter_map(|l| match l {
                Line::MenuListItem { selected, .. } => Some(*selected),
                _ => None,
            })
            .collect();
        assert_eq!(selected_in_zero, vec![true, false, false]);

        let at_two = Mkui::new().child(
            MenuList::new()
                .item("A")
                .item("B")
                .item("C")
                .selected_index(2),
        );
        let mut layout_two = Vec::new();
        let mut buttons_two = Vec::new();
        walk_tree(at_two.tree(), &mut layout_two, &mut buttons_two);
        let selected_in_two: Vec<bool> = layout_two
            .iter()
            .filter_map(|l| match l {
                Line::MenuListItem { selected, .. } => Some(*selected),
                _ => None,
            })
            .collect();
        assert_eq!(selected_in_two, vec![false, false, true]);
    }
}
