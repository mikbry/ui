//! Smoke tests for the `mkui` bridge crate's feature/backend selection.
//!
//! The bridge crate dispatches to a concrete backend (`mkui-web`,
//! `mkui-console`, …) based on Cargo features. These tests pin down the
//! observable contract for each configuration the workspace exercises:
//!
//! - **No backend feature** — `Mkui::new()` returns an `Initialization`
//!   error so library consumers get a clear diagnostic instead of a link
//!   error or silent no-op.
//! - **`console` feature** — `Mkui::new()` succeeds on a host without a
//!   real TTY (CI), and child components are accepted via the same
//!   builder API the other backends expose.
//!
//! The `web` backend path requires a browser DOM at runtime and is
//! validated by the web showcase; this file only asserts what `cargo test`
//! can exercise natively.

use mkui::prelude::*;

#[cfg(not(any(feature = "web", feature = "console", feature = "wgpu")))]
#[test]
fn without_a_backend_feature_init_reports_a_clear_error() {
    let err = match Mkui::new() {
        Ok(_) => panic!("no backend feature should fail to init"),
        Err(e) => e,
    };
    assert!(matches!(err, MkuiError::Initialization(_)));
    assert!(
        err.to_string().contains("No mkui backend feature enabled"),
        "error message should name the missing feature: {err}",
    );
}

#[cfg(feature = "console")]
#[test]
fn console_backend_accepts_a_component_tree() {
    let app = Mkui::new().expect("console init should succeed without a real TTY");
    // Building a small tree must not panic on either downcast or on the
    // `child(...)` dispatch in the bridge's cfg-gated impl.
    // `row` is not in the Tier 1 token set; the runtime parser would reject
    // it. The bridge smoke test uses `flex` instead — a real T1 token — so
    // the build path exercises the lowering registry as well as the
    // class-parser gate.
    let _app = app
        .child(Text::new("hello"))
        .child(View::new().class("flex").child(Button::new("ok")));
}

/// #311: the bridge's `Mkui::with_slug_font` must actually forward to the
/// wgpu backend's own rejection logic, not swallow or bypass it — a
/// `FontId` that isn't a `TextRenderClass::Slug` face on the supplied text
/// system must fail here exactly as it does calling
/// `mkui_wgpu::Mkui::with_slug_font` directly. No font fixture needed: the
/// rejection fires before any SFNT bytes would be touched.
#[cfg(all(feature = "wgpu", feature = "slug"))]
#[test]
fn with_slug_font_forwards_the_wgpu_backends_rejection() {
    let text_system = mkui::CompositeTextSystem::new();
    let result = Mkui::new()
        .expect("wgpu init should succeed headlessly")
        .with_slug_font(text_system, mkui::FontId::BITMAP);
    let message = match result {
        Ok(_) => panic!("FontId::BITMAP is never a Slug-class face"),
        Err(e) => e.to_string(),
    };
    assert!(
        message.contains("with_slug_font"),
        "the bridge must surface the backend's own rejection message: {message}"
    );
}

#[test]
fn prelude_re_exports_core_contract_types() {
    // Compile-time check: the bridge prelude must expose the shared
    // contract so downstream crates don't have to depend on `mkui-core`
    // directly.
    fn assert_send_or_anything<T>() {}
    assert_send_or_anything::<Button>();
    assert_send_or_anything::<View>();
    assert_send_or_anything::<Text>();
    assert_send_or_anything::<MkuiError>();
    assert_send_or_anything::<ThemeMode>();
}
