//! `info_list` — vertical list of lines under a title inside a panel.
//!
//! The familiar "mode / status / history" card: a [`panel`](super::panel) with
//! a title row followed by one body line per entry. Kept for existing overlay
//! code; new panels should compose [`titled_panel`](super::titled_panel) +
//! [`label`](super::label) directly.

use crate::theme::WgpuTheme;
use crate::types::{Point, Rect, Scene, Size};

use super::{label, panel};

/// Vertical list of lines under a title inside a panel — the familiar
/// "mode / status / history" card. Kept for existing overlay code; new
/// panels should compose [`titled_panel`](super::titled_panel) +
/// [`label`](super::label) directly.
pub fn info_list(
    scene: &mut Scene,
    rect: Rect,
    title: &str,
    lines: &[impl AsRef<str>],
    theme: &WgpuTheme,
) {
    let content = panel(scene, rect, theme.panel);
    label(
        scene,
        Rect::new(
            content.origin,
            Size::new(content.size.width, theme.title_style.line_height_px),
        ),
        title,
        theme.title_style,
    );

    let mut y = content.origin.y + theme.title_style.line_height_px + 6.0;
    for line in lines {
        label(
            scene,
            Rect::new(
                Point::new(content.origin.x, y),
                Size::new(content.size.width, theme.body_style.line_height_px),
            ),
            line.as_ref(),
            theme.body_style,
        );
        y += theme.body_style.line_height_px + 2.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Primitive;

    #[test]
    fn info_list_builds_panel_and_text() {
        let mut scene = Scene::new(Size::new(800.0, 600.0));
        let theme = WgpuTheme::default();
        info_list(
            &mut scene,
            Rect::new(Point::new(20.0, 20.0), Size::new(260.0, 140.0)),
            "Status",
            &["Plan view", "Undo Ctrl+Z"],
            &theme,
        );

        assert!(scene.primitives.len() >= 4);
        assert!(matches!(scene.primitives[0], Primitive::Shadow(_)));
        assert!(scene
            .primitives
            .iter()
            .any(|primitive| matches!(primitive, Primitive::Text(_))));
    }
}
