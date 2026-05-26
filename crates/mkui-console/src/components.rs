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
/// [`ConsoleButton`] in the parent app by index.
#[derive(Clone, Debug, PartialEq)]
pub enum Line {
    Heading(String),
    Body(String),
    Muted(String),
    Spacer,
    Button(usize),
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
            NodeKind::Root | NodeKind::Custom { .. } => {
                // Root is unreachable here (we started at its children). A
                // custom node has no built-in console rendering — Sprint 6+
                // extension registry will define one. For now we recurse
                // into any children so a custom container still surfaces
                // its descendents.
                walk_children(tree, &node.children, layout, buttons);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mkui_core::components::{Button, Mkui, Text, View};
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
}
