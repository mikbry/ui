//! High-level [`Mkui`] entry point for the console backend.
//!
//! Sprint 4: the backend now owns an `mkui_core::Mkui` (which itself wraps
//! an `mkui_runtime::AppTree`). The navigation loop reads the runtime tree,
//! looks up actions through the tree's `ActionRegistry`, and fires them on
//! Enter/Space — closures register dirty bits via `RuntimeCtx`, the renderer
//! observes them.

use crossterm::{
    event::{self, Event, KeyCode, KeyEvent},
    style::Stylize,
};
use mkui_core::components::Component;
use mkui_runtime::ButtonVariant;

use crate::app::ConsoleApp;
use crate::components::{walk_tree, Line, Selectable};
use crate::renderer::ConsoleRenderer;

/// Console backend for the shared `mkui-core` component tree.
pub struct Mkui {
    app: ConsoleApp,
    core: mkui_core::components::Mkui,
    layout: Vec<Line>,
    /// Every interactive control (`Button` or `MenuList` item) in
    /// tree-walk order. `self.app`'s cursor is an index into this one
    /// `Vec`, so Up/Down/Enter navigate and activate whichever kind of
    /// control the cursor currently lands on (#189 round 3).
    selectables: Vec<Selectable>,
}

impl Mkui {
    pub fn new() -> std::io::Result<Self> {
        Ok(Self {
            app: ConsoleApp::new()?,
            core: mkui_core::components::Mkui::new(),
            layout: Vec::new(),
            selectables: Vec::new(),
        })
    }

    /// Wrap an existing `mkui_core::Mkui` (and its `AppTree`) so the
    /// console backend can run it. Used by `mkui-c` / `mkui-py` to hand
    /// their already-built tree to the real interactive loop without
    /// having to rebuild via `.child(...)`.
    pub fn from_core(core: mkui_core::components::Mkui) -> std::io::Result<Self> {
        Ok(Self {
            app: ConsoleApp::new()?,
            core,
            layout: Vec::new(),
            selectables: Vec::new(),
        })
    }

    /// Matches the web `Mkui` API: append a component to the tree.
    pub fn child(mut self, child: impl Component + 'static) -> Self {
        self.core = self.core.child(child);
        self
    }

    pub fn run(mut self) -> std::io::Result<()> {
        self.build_layout();

        if self.selectables.is_empty() {
            // No interactive controls at all (not even a MenuList with no
            // items) — render once without the "Press q/Esc" footer and
            // return. The footer would advertise keys the program never
            // reads. A MenuList with items always has *something* here
            // now, so a menu-only tree takes the interactive path below
            // instead of this early return (#189 round 3).
            return self.render(false);
        }

        // Seed the cursor from a `MenuList`'s construction-time
        // `selected_index` (falling back to 0, the existing button-only
        // default) exactly once, before the loop starts. Later
        // `build_layout()` refreshes (after confirming an item) must not
        // reset wherever the user has since navigated to.
        let seed = self
            .selectables
            .iter()
            .position(|s| matches!(s, Selectable::MenuItem(item) if item.initial))
            .unwrap_or(0);
        self.app.set_selected_index(seed);

        self.app.renderer().enter_alt_screen()?;

        loop {
            let current_size = self.app.renderer().size();
            if current_size != self.app.last_terminal_size() {
                self.app.set_last_terminal_size(current_size);
                self.app.renderer().clear_all()?;
            }

            self.render(true)?;

            let evt = event::read()?;
            match evt {
                Event::Key(KeyEvent { code, .. }) => match code {
                    KeyCode::Left | KeyCode::Up => {
                        self.app.select_prev(self.selectables.len());
                    }
                    KeyCode::Right | KeyCode::Down => {
                        self.app.select_next(self.selectables.len());
                    }
                    KeyCode::Enter | KeyCode::Char(' ') => {
                        if let Some(selectable) = self.selectables.get(self.app.selected_index()) {
                            if let Some(action_id) = selectable.on_activate() {
                                // Fire through the tree's action registry,
                                // **capture** the returned `RuntimeCtx`,
                                // and propagate its dirty bit to the tree.
                                // Dropping the ctx (the Codex round-7
                                // anti-pattern) means action-driven state
                                // changes never trigger a redraw — the
                                // console loop re-renders every iteration
                                // so we'd survive, but the substrate's
                                // contract is broken without this hop.
                                let ctx = self.core.tree().actions().fire(action_id);
                                if ctx.is_dirty() {
                                    self.core.tree_mut().mark_dirty();
                                }
                                // Re-walk the tree so any structural changes
                                // the action made (future scope — toggling
                                // child visibility, etc.) surface immediately
                                // rather than waiting for the next event.
                                // The cursor itself lives in `self.app`, not
                                // in the rebuilt `self.selectables`, so this
                                // refresh does not reset it.
                                self.build_layout();
                            }
                        }
                    }
                    KeyCode::Char('q') | KeyCode::Esc => break,
                    _ => {}
                },
                Event::Resize(_, _) => continue,
                _ => {}
            }
        }

        self.app.renderer().leave_alt_screen()?;
        Ok(())
    }

    fn build_layout(&mut self) {
        let mut layout = Vec::new();
        let mut selectables = Vec::new();
        walk_tree(self.core.tree(), &mut layout, &mut selectables);
        self.layout = layout;
        self.selectables = selectables;
    }

    fn render(&self, interactive: bool) -> std::io::Result<()> {
        let renderer: &ConsoleRenderer = self.app.renderer();
        let (width, height) = renderer.size();
        let width = width as usize;
        let height = height as usize;
        let (last_w, last_h) = self.app.last_terminal_size();

        let clear_width = width.max(last_w as usize);
        let clear_height = height.max(last_h as usize);
        renderer.paint_blank(clear_width, clear_height)?;

        let mut current_row: u16 = 2;
        for line in &self.layout {
            match line {
                Line::Heading(text) => {
                    renderer.print_styled(2, current_row, text.clone().white().bold())?;
                    current_row += 1;
                }
                Line::Body(text) => {
                    renderer.print_styled(2, current_row, text.clone().white())?;
                    current_row += 1;
                }
                Line::Muted(text) => {
                    renderer.print_styled(2, current_row, text.clone().dark_grey())?;
                    current_row += 1;
                }
                Line::Spacer => current_row += 1,
                Line::Button(index) => {
                    if let Some(Selectable::Button(button)) = self.selectables.get(*index) {
                        let is_selected = self.app.selected_index() == *index;
                        let text = format!("[ {} ]", button.label);
                        match (is_selected, button.variant) {
                            (true, ButtonVariant::Primary) => renderer.print_styled(
                                4,
                                current_row,
                                text.white().on_blue().bold(),
                            )?,
                            (true, ButtonVariant::Secondary) => renderer.print_styled(
                                4,
                                current_row,
                                text.black().on_grey().bold(),
                            )?,
                            (true, ButtonVariant::Destructive) => renderer.print_styled(
                                4,
                                current_row,
                                text.white().on_red().bold(),
                            )?,
                            (true, ButtonVariant::Outline) => {
                                renderer.print_styled(4, current_row, text.blue().bold())?
                            }
                            (true, ButtonVariant::Ghost) => {
                                renderer.print_styled(4, current_row, text.blue().bold())?
                            }
                            (true, ButtonVariant::Link) => renderer.print_styled(
                                4,
                                current_row,
                                text.blue().bold().underlined(),
                            )?,
                            (false, ButtonVariant::Primary) => {
                                renderer.print_styled(4, current_row, text.white().on_blue())?
                            }
                            (false, ButtonVariant::Secondary) => {
                                renderer.print_styled(4, current_row, text.dark_grey())?
                            }
                            (false, ButtonVariant::Destructive) => {
                                renderer.print_styled(4, current_row, text.white().on_dark_red())?
                            }
                            (false, ButtonVariant::Outline) => {
                                renderer.print_styled(4, current_row, text.white())?
                            }
                            (false, ButtonVariant::Ghost) => {
                                renderer.print_styled(4, current_row, text.dark_grey())?
                            }
                            (false, ButtonVariant::Link) => {
                                renderer.print_styled(4, current_row, text.white().underlined())?
                            }
                            // `ButtonVariant` is `#[non_exhaustive]` — future
                            // variants fall back to the Primary style until
                            // their own arm lands.
                            (true, _) => renderer.print_styled(
                                4,
                                current_row,
                                text.white().on_blue().bold(),
                            )?,
                            (false, _) => {
                                renderer.print_styled(4, current_row, text.white().on_blue())?
                            }
                        }
                        current_row += 1;
                    }
                }
                Line::MenuListItem(index) => {
                    if let Some(Selectable::MenuItem(item)) = self.selectables.get(*index) {
                        // Acceptance criterion #2 of #189: the current
                        // selection must be visually distinct. Decided here,
                        // against the live cursor — never baked into the
                        // `Line` at walk time — so it reflects every
                        // Up/Down/gamepad-equivalent move the user has made,
                        // not just the construction-time default. A leading
                        // marker (">"/" ") keeps the row legible even
                        // without color (no-color terminals, piped output);
                        // the inverse-video style is the color-terminal cue
                        // on top of that.
                        let is_selected = self.app.selected_index() == *index;
                        if is_selected {
                            let text = format!("> {}", item.label);
                            renderer.print_styled(
                                4,
                                current_row,
                                text.black().on_white().bold(),
                            )?;
                        } else {
                            let text = format!("  {}", item.label);
                            renderer.print_styled(4, current_row, text.dark_grey())?;
                        }
                        current_row += 1;
                    }
                }
            }
        }

        if interactive {
            let instructions = "↑↓/←→: Select | Space/Enter: Activate | q: Quit";
            let instr_y = (height.saturating_sub(1)).min(current_row as usize + 2);
            renderer.print_footer(instr_y as u16, instructions)?;
        }

        renderer.flush()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mkui_core::components::{Button as CoreButton, MenuList, Text, View};
    use mkui_core::headless::TextVariant;
    use std::cell::RefCell;
    use std::rc::Rc;

    #[test]
    fn build_layout_renders_a_realistic_showcase_tree() {
        let mut app = Mkui::new().expect("init mkui-console");
        app = app.child(
            View::new()
                .child(Text::new("Hello").variant(TextVariant::Heading1))
                .child(Text::new("subtitle").variant(TextVariant::Caption))
                .child(
                    View::new()
                        .child(CoreButton::new("Primary").variant(ButtonVariant::Primary))
                        .child(CoreButton::new("Ghost").variant(ButtonVariant::Ghost)),
                ),
        );

        app.build_layout();

        assert_eq!(app.selectables.len(), 2);
        assert_eq!(app.selectables[0].label(), "Primary");
        assert_eq!(app.selectables[1].label(), "Ghost");
        assert!(matches!(
            &app.selectables[0],
            Selectable::Button(b) if b.variant == ButtonVariant::Primary
        ));

        let button_lines: Vec<&Line> = app
            .layout
            .iter()
            .filter(|l| matches!(l, Line::Button(_)))
            .collect();
        assert_eq!(button_lines.len(), 2);

        assert!(app
            .layout
            .iter()
            .any(|l| matches!(l, Line::Heading(t) if t == "Hello")));
        assert!(app
            .layout
            .iter()
            .any(|l| matches!(l, Line::Muted(t) if t == "subtitle")));
    }

    /// Round-3 review on #189: prior tests only compared two separately-
    /// constructed *static* menus, so they never proved input could move
    /// the highlight on an already-rendered menu. This test drives one
    /// `Mkui` instance's actual navigation path — `ConsoleApp::select_next`/
    /// `select_prev`, exactly what `run()`'s key-event match arms call —
    /// and checks the SAME cursor `render()` reads from
    /// (`self.app.selected_index()` compared against each
    /// `Line::MenuListItem`'s index) moves with it, then confirms the
    /// highlighted item and checks the right index reached `on_select`.
    #[test]
    fn menu_list_input_moves_the_rendered_highlight_and_confirms() {
        let fired = Rc::new(RefCell::new(None));
        let fired_in = Rc::clone(&fired);

        let mut app = Mkui::new().expect("init mkui-console");
        app = app.child(
            MenuList::new()
                .item("New Game")
                .item("Continue")
                .item("Quit")
                .on_select(move |index| *fired_in.borrow_mut() = Some(index)),
        );

        app.build_layout();
        app.app.set_selected_index(0);

        // Helper: which global `Selectable` index does `render()` currently
        // treat as selected, per `Line::MenuListItem` order?
        fn highlighted_label(app: &Mkui) -> &str {
            let cursor = app.app.selected_index();
            app.selectables[cursor].label()
        }

        assert_eq!(highlighted_label(&app), "New Game");

        // Same key handling `run()`'s `KeyCode::Down` arm uses.
        app.app.select_next(app.selectables.len());
        assert_eq!(
            highlighted_label(&app),
            "Continue",
            "Down must move the rendered highlight to the next row"
        );

        app.app.select_next(app.selectables.len());
        assert_eq!(highlighted_label(&app), "Quit");

        app.app.select_next(app.selectables.len());
        assert_eq!(
            highlighted_label(&app),
            "New Game",
            "Down from the last row must wrap back to the first"
        );

        app.app.select_prev(app.selectables.len());
        assert_eq!(
            highlighted_label(&app),
            "Quit",
            "Up from the first row must wrap to the last"
        );

        // Land on "Continue" and confirm it — same lookup + fire sequence
        // as `run()`'s `KeyCode::Enter` arm.
        app.app.select_next(app.selectables.len());
        app.app.select_next(app.selectables.len());
        assert_eq!(highlighted_label(&app), "Continue");

        let selectable = &app.selectables[app.app.selected_index()];
        let action = selectable
            .on_activate()
            .expect("the highlighted item must have an action when on_select is set");
        let ctx = app.core.tree().actions().fire(action);
        assert!(ctx.is_dirty(), "confirming must mark the tree dirty (§7)");
        assert_eq!(
            *fired.borrow(),
            Some(1),
            "activating the highlighted row must fire on_select with THAT row's index, \
             not the construction-time default"
        );
    }

    /// Round-3 review: "a menu-only application renders once and exits."
    /// `run()` gates the interactive loop on `self.selectables.is_empty()`
    /// — this proves that gate is false for a `MenuList`-only tree (no
    /// `Button` anywhere), so such a tree takes the interactive path
    /// instead of the single-render early return.
    #[test]
    fn menu_only_tree_has_selectables_so_run_would_enter_the_interactive_loop() {
        let mut app = Mkui::new().expect("init mkui-console");
        app = app.child(MenuList::new().item("New Game").item("Quit"));

        app.build_layout();

        assert!(
            !app.selectables.is_empty(),
            "a MenuList-only tree must not hit run()'s empty-selectables \
             single-render early return"
        );
        assert_eq!(app.selectables.len(), 2);
    }

    /// The interactive loop seeds its cursor from a `MenuList`'s
    /// construction-time `selected_index` (not always 0) before the first
    /// render — this is the same `position(...).unwrap_or(0)` computation
    /// `run()` does right after its first `build_layout()`.
    #[test]
    fn initial_cursor_seed_honors_menu_lists_construction_time_selection() {
        let mut app = Mkui::new().expect("init mkui-console");
        app = app.child(
            MenuList::new()
                .item("New Game")
                .item("Continue")
                .item("Quit")
                .selected_index(1),
        );

        app.build_layout();

        let seed = app
            .selectables
            .iter()
            .position(|s| matches!(s, Selectable::MenuItem(item) if item.initial))
            .unwrap_or(0);
        assert_eq!(seed, 1, "the seed must land on \"Continue\", not row 0");
    }
}
