//! Winit `ApplicationHandler` shell that drives the mkui-wgpu renderer.
//!
//! Sprint 5 (issue #56): the app shell now drives the
//! [`mkui_runtime::AppTree`] declarative path end-to-end. On each frame
//! it walks the tree into a `Scene`, hands the scene to the renderer,
//! and routes pointer input through the per-frame hit-test vector. The
//! `Scene`-only constructor (`WgpuApp::new`) is retained as the
//! low-level escape hatch — see [`crate::Mkui::with_scene`] and ADR
//! 0006 §"`with_scene` as the retained low-level escape hatch".

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use mkui_text::{BitmapTextSystem, TextSystem};

use crate::bridge::WgpuRendererRegistry;
use crate::theme::WgpuTheme;
use crate::types::{Scene, Size};

#[cfg(not(target_arch = "wasm32"))]
use winit::{
    application::ApplicationHandler,
    event::WindowEvent,
    event_loop::ActiveEventLoop,
    window::{Window, WindowAttributes, WindowId},
};

#[cfg(not(target_arch = "wasm32"))]
use crate::pointer::{left_button_state, PointerState};
#[cfg(not(target_arch = "wasm32"))]
use crate::render::{RenderOutcome, Renderer};
#[cfg(not(target_arch = "wasm32"))]
use crate::walker::{walk_app_tree_cached, HitTestEntry, WalkCache, WalkOptions};
#[cfg(not(target_arch = "wasm32"))]
use mkui_runtime::RuntimeSignal;
#[cfg(not(target_arch = "wasm32"))]
use winit::event::{ElementState, KeyEvent};
#[cfg(not(target_arch = "wasm32"))]
use winit::keyboard::{KeyCode, PhysicalKey};

/// Application shell. Owns the active scene + text system (for the
/// scene-only escape hatch path) and, when constructed via
/// [`WgpuApp::with_app_tree`], the runtime [`mkui_core::components::Mkui`]
/// + the [`WgpuRendererRegistry`] for custom-component dispatch.
///
/// Native targets additionally hold the `Renderer` state and the
/// per-frame hit-test buffer. Both are `Option` so the same struct shape
/// compiles on wasm consumers that don't pull in winit.
pub struct WgpuApp {
    scene: Scene,
    text_system: Arc<dyn TextSystem>,
    theme: WgpuTheme,
    /// Either a runtime tree (declarative path) or `None` (raw-scene
    /// escape hatch). When `Some`, `Renderer::render` is called against
    /// a tree-derived scene the app rebuilds per frame; when `None`, the
    /// original `scene` field is rendered verbatim.
    core: Option<Rc<RefCell<mkui_core::components::Mkui>>>,
    registry: Option<WgpuRendererRegistry>,
    #[cfg(not(target_arch = "wasm32"))]
    state: Option<WgpuAppState>,
    #[cfg(not(target_arch = "wasm32"))]
    window_title: String,
    #[cfg(not(target_arch = "wasm32"))]
    headless: bool,
    /// Most recent per-frame hit-test list. Replaced wholesale on each
    /// rebuild via [`crate::walker::walk_app_tree_cached`]'s `WalkOutput`;
    /// the input router reads this slice (no `&mut` is ever handed to a
    /// renderer or action closure).
    #[cfg(not(target_arch = "wasm32"))]
    hit_tests: Vec<HitTestEntry>,
    /// Per-subtree render cache (#229) backing [`WgpuApp::try_rebuild_scene_from_tree`].
    /// Lives for the whole app, not just one rebuild — see [`WalkCache`]'s
    /// own docs for what invalidates it (an `AppTree::epoch` bump, or a
    /// theme/viewport change).
    #[cfg(not(target_arch = "wasm32"))]
    walk_cache: WalkCache,
    #[cfg(not(target_arch = "wasm32"))]
    pointer: PointerState,
    /// True until the freshly-created window has produced its first
    /// successfully-`Drawn` frame. While set, a `Skipped` render outcome
    /// (surface not yet ready) reschedules another redraw instead of
    /// idling — otherwise the window stays blank-gray until the user
    /// resizes or interacts (#93 round-N+1).
    #[cfg(not(target_arch = "wasm32"))]
    first_paint_pending: bool,
    /// Remaining `Skipped`→retry reschedules allowed during first paint.
    /// Caps the retry loop so a genuinely occluded window doesn't spin
    /// redraws forever.
    #[cfg(not(target_arch = "wasm32"))]
    first_paint_skip_retries: u8,
    /// Resize-active redraw pump (#99). Armed to [`RESIZE_REDRAW_PUMP_TICKS`]
    /// on `Resized` + `ScaleFactorChanged`; `about_to_wait` requests redraws
    /// while armed, and successful `RenderOutcome::Drawn` frames decrement it
    /// until idle windows return to quiescence. Deliberately independent of
    /// the first-paint state machine — `RenderOutcome::Drawn` does NOT clear
    /// it (a drawn frame proves one frame presented, not that the live-resize
    /// gesture has gone quiet). See ADR 0006 §"Resize-active redraw pump".
    #[cfg(not(target_arch = "wasm32"))]
    resize_redraw_pending: u8,
    /// Monotonic frame counter, incremented once per `about_to_wait` tick
    /// (#101). Backs the active-resize-window guard
    /// ([`WgpuApp::in_active_resize_window`]) so the `CursorMoved` M2 bridge
    /// can be gated on a pump-state-independent timeout rather than the
    /// circular `resize_redraw_pending > 0` predicate (a drained pump would
    /// otherwise read as "not resizing" exactly when cursor-driven redraws
    /// are needed). Saturates rather than wraps — a window that never closes
    /// would take ~2 years at 60Hz to reach `u32::MAX`, well past any session.
    #[cfg(not(target_arch = "wasm32"))]
    frame_counter: u32,
    /// Frame index of the most recent resize arm (#101). `None` until the
    /// first `Resized` / `ScaleFactorChanged`; thereafter stamped to
    /// `frame_counter` on every arm. The active-resize window is open while
    /// `frame_counter - last_resize_frame < RESIZE_ACTIVE_WINDOW_FRAMES`.
    #[cfg(not(target_arch = "wasm32"))]
    last_resize_frame: Option<u32>,
    /// User-facing UI-scale (text/size) multiplier (#179), applied on top of
    /// the OS HiDPI `scale_factor` wherever a physical size is converted to
    /// the logical-pixel viewport the walker lays out against — see
    /// [`WgpuApp::set_ui_scale`] and ADR 0006 §"UI-scale setting". Not
    /// `cfg`-gated: it carries no winit dependency, so a future wasm/mobile
    /// host (#180) can read/write it even before that target gets its own
    /// resize plumbing.
    ui_scale: f32,
    /// Most recent `(physical_width, physical_height, os_scale_factor)` an
    /// embedding host pushed via [`WgpuApp::set_surface_size`] (#179).
    /// `None` until the first call. Lets [`WgpuApp::set_ui_scale`] recompute
    /// the viewport on a UI-scale change alone, without the host having to
    /// re-report its surface size. Irrelevant on the winit-owned path (that
    /// path re-queries `state.window` directly instead).
    #[cfg(not(target_arch = "wasm32"))]
    last_surface_size: Option<(u32, u32, f64)>,
    /// Set by [`WgpuApp::set_ui_scale`] (review follow-up on #179):
    /// `about_to_wait` requests one redraw the next time it ticks, then
    /// clears this. A `ui_scale` change doesn't touch the
    /// renderer/surface config, so arming the full resize-jerk pump
    /// (#99/#101 — 60 ticks plus the macOS `CursorMoved` bridge window)
    /// would be using the wrong tool purely to get one redraw scheduled;
    /// this is a dedicated, minimal flag instead. Plain data — unlike a
    /// direct `state.window.request_redraw()` call, it's assertable from
    /// a unit test with no live window at all.
    #[cfg(not(target_arch = "wasm32"))]
    redraw_requested: bool,
}

/// Lower bound [`WgpuApp::set_ui_scale`] clamps to — small enough to still
/// read as "smaller" without shrinking controls below a usable hit target.
const MIN_UI_SCALE: f32 = 0.5;

/// Upper bound [`WgpuApp::set_ui_scale`] clamps to. Mirrors the 1x/2x/3x
/// HiDPI range the resolution acceptance criterion (#179) names, as a
/// reasonable ceiling for the same reason: `walker.rs`'s fixed-size atoms
/// (e.g. `BUTTON_HEIGHT_PX`) aren't clamped to `content_width`, so pushing
/// the effective viewport arbitrarily small by stacking a large `ui_scale`
/// onto a narrow window eventually overflows it for long enough content —
/// see `menu_content_stays_within_the_viewport_across_the_179_acceptance_matrix`
/// in `walker.rs` for where that boundary actually sits for realistic
/// content (not an unbounded one), and #194 (an existing, orthogonal gap)
/// for the scroll-area fix content long enough to need it is waiting on.
const MAX_UI_SCALE: f32 = 3.0;

/// Cap on first-paint `Skipped`→retry reschedules before giving up, so a
/// genuinely occluded or never-ready surface doesn't spin redraws forever
/// (#93 round-N+1).
#[cfg(not(target_arch = "wasm32"))]
const FIRST_PAINT_MAX_SKIP_RETRIES: u8 = 8;

/// Number of `about_to_wait` redraw ticks the resize-active pump drives
/// after each `Resized` / `ScaleFactorChanged` event (#99). Bounds the
/// pump so it bridges the Metal swapchain transition during live-resize
/// without spinning redraws on an idle window. Analogous to
/// [`FIRST_PAINT_MAX_SKIP_RETRIES`] — a cap that decays cleanly.
///
/// Tuned across Sprint 6.5 (#99 → #100) as operator visual verify narrowed
/// the residual symptom:
/// - **4** (round-N F4): kept up with slow drags but exhausted faster than
///   `Resized` events re-armed it during *fast* macOS live-resize, so the
///   jerk persisted on quick right→left / bottom→top gestures.
/// - **60** (round-N+1 BLOCK): matched an upstream wgpu+winit reference's
///   proven accumulation-frame cap; symptom dropped from a full jerk to a
///   small residual vibration on fast drags. **This is the value shipped.**
/// - **120** (round-N+2): tried to close the residual by doubling the
///   active-redraw window — but operator visual verify found 120 *worse*
///   than 60, refuting the "more budget = smoother" hypothesis. Likely the
///   pump over-fires redraws during fast drag, queueing frames against stale
///   surface configs and saturating the swapchain-transition cost. Reverted.
///
/// 60 hits the balance between "enough redraws to bridge swapchain
/// transitions" and "few enough not to saturate the GPU". After the last
/// `Resized` the pump decays in ~60 presented frames (~1s at 60Hz) back to
/// idle quiescence. The residual fast-drag vibration at 60 is NOT pump
/// exhaustion (refuted by the 120 result); it is tracked as #101 for a
/// shape change — a `CursorMoved`-armed bridge during the active-resize
/// window — not a further cap tune.
#[cfg(not(target_arch = "wasm32"))]
const RESIZE_REDRAW_PUMP_TICKS: u8 = 60;

/// Length, in `about_to_wait` frames, of the active-resize window that gates
/// the `CursorMoved` M2 redraw bridge (#101). The window opens on every
/// `Resized` / `ScaleFactorChanged` arm (stamped into `last_resize_frame`) and
/// closes this many frames later — independent of the resize pump's budget.
///
/// This guard is deliberately **not** `resize_redraw_pending > 0`: that
/// predicate is circular (a drained pump reads as "not resizing" exactly when
/// the residual fast-shrink vibration needs cursor-driven redraws — Codex
/// 2026-06-09 #101 review P1#2). A frame-counter timeout decouples "are we in
/// a live-resize gesture?" from "does the pump still have budget?".
///
/// 60 frames (~1s at 60Hz) mirrors [`RESIZE_REDRAW_PUMP_TICKS`]: long enough to
/// span the gap between successive `Resized` events during a fast drag (so the
/// cursor bridge stays live across the whole gesture), short enough that idle
/// pointer motion well after the drag stops does not resurrect redraws —
/// preserving the idle-quiescence invariant a UI framework requires.
#[cfg(not(target_arch = "wasm32"))]
const RESIZE_ACTIVE_WINDOW_FRAMES: u32 = 60;

#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug)]
struct WgpuAppState {
    window: Arc<Window>,
    renderer: Renderer,
}

impl WgpuApp {
    pub fn new(scene: Scene) -> Self {
        Self {
            scene,
            text_system: Arc::new(BitmapTextSystem::new()),
            theme: WgpuTheme::default(),
            core: None,
            registry: None,
            #[cfg(not(target_arch = "wasm32"))]
            state: None,
            #[cfg(not(target_arch = "wasm32"))]
            window_title: "mkui".to_string(),
            #[cfg(not(target_arch = "wasm32"))]
            headless: false,
            #[cfg(not(target_arch = "wasm32"))]
            hit_tests: Vec::new(),
            #[cfg(not(target_arch = "wasm32"))]
            walk_cache: WalkCache::new(),
            #[cfg(not(target_arch = "wasm32"))]
            pointer: PointerState::new(),
            #[cfg(not(target_arch = "wasm32"))]
            first_paint_pending: true,
            #[cfg(not(target_arch = "wasm32"))]
            first_paint_skip_retries: FIRST_PAINT_MAX_SKIP_RETRIES,
            #[cfg(not(target_arch = "wasm32"))]
            resize_redraw_pending: 0,
            #[cfg(not(target_arch = "wasm32"))]
            frame_counter: 0,
            #[cfg(not(target_arch = "wasm32"))]
            last_resize_frame: None,
            ui_scale: 1.0,
            #[cfg(not(target_arch = "wasm32"))]
            last_surface_size: None,
            #[cfg(not(target_arch = "wasm32"))]
            redraw_requested: false,
        }
    }

    /// Build the app with an explicit text system. Renderer-side hosts that
    /// need to swap implementations (bitmap → Slug) construct via this entry
    /// rather than `new`.
    pub fn with_text_system(scene: Scene, text_system: Arc<dyn TextSystem>) -> Self {
        let mut app = Self::new(scene);
        app.text_system = text_system;
        app
    }

    /// Build a declarative-path app from a runtime [`mkui_core::components::Mkui`]
    /// plus a custom-component registry. The event loop rebuilds the scene
    /// from the tree on every dirty frame (Sprint 4 substrate contract, ADR
    /// 0006) — within that rebuild, an unchanged built-in subtree is
    /// spliced from a cache rather than re-walked (#229, ADR 0006's
    /// "Update (#229)" section).
    pub fn with_app_tree(
        core: mkui_core::components::Mkui,
        registry: WgpuRendererRegistry,
    ) -> Self {
        let mut app = Self::new(Scene::new(Size::new(1280.0, 720.0)));
        app.core = Some(Rc::new(RefCell::new(core)));
        app.registry = Some(registry);
        app
    }

    /// Build a declarative-path app that **retains an explicitly supplied text
    /// system** (#66). Identical to [`WgpuApp::with_app_tree`] except the
    /// renderer draws against `text_system` instead of the bitmap default.
    ///
    /// This is the declarative analogue of [`WgpuApp::with_text_system`]: a
    /// composite text system (bitmap + Slug + outline) handed in here survives
    /// `run` and every per-frame scene rebuild — the app never silently swaps
    /// in the bitmap default. See [`crate::Mkui::from_core_with_text_system`].
    pub fn with_app_tree_and_text_system(
        core: mkui_core::components::Mkui,
        registry: WgpuRendererRegistry,
        text_system: Arc<dyn TextSystem>,
    ) -> Self {
        let mut app = Self::with_app_tree(core, registry);
        app.text_system = text_system;
        app
    }

    pub fn scene(&self) -> &Scene {
        &self.scene
    }

    pub fn set_scene(&mut self, scene: Scene) {
        self.scene = scene;
    }

    pub fn text_system(&self) -> &Arc<dyn TextSystem> {
        &self.text_system
    }

    /// Swap the active text system. Also flushes the per-subtree
    /// [`WalkCache`] (#229) that backs the declarative-path rebuild: a
    /// cached subtree's `Text` primitives were
    /// measured (line count, wrapped height) against the *previous* text
    /// system, and nothing about that measurement is visible in
    /// `AppTree::subtree_revision` — reusing it after the swap would paint
    /// stale geometry under the new system's metrics.
    pub fn set_text_system(&mut self, text_system: Arc<dyn TextSystem>) {
        self.text_system = text_system;
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.walk_cache = WalkCache::new();
        }
    }

    pub fn theme(&self) -> &WgpuTheme {
        &self.theme
    }

    /// Swap the active theme. A no-op if `theme` is identical to the
    /// current one (`WgpuTheme: PartialEq`). Otherwise, when the app owns
    /// a runtime tree, marks it dirty so the next [`WgpuApp::update_frame`]
    /// (or the winit `RedrawRequested` dirty check) picks up the new
    /// theme on its next walk — a theme swap is not itself a tree
    /// mutation, so without this it would silently not redraw (#177).
    pub fn set_theme(&mut self, theme: WgpuTheme) {
        if self.theme == theme {
            return;
        }
        self.theme = theme;
        if let Some(core) = self.core.as_ref() {
            core.borrow_mut().tree_mut().mark_dirty();
        }
    }

    /// Current UI-scale (text/size) multiplier (#179). `1.0` is the
    /// default/no-op scale. See [`WgpuApp::set_ui_scale`].
    pub fn ui_scale(&self) -> f32 {
        self.ui_scale
    }

    /// Set the user-facing UI-scale multiplier, folded into the same
    /// logical-pixel viewport the OS HiDPI `scale_factor` divides into (ADR
    /// 0006 §"UI-scale setting"): doubling it halves `Scene::viewport` for a
    /// fixed physical surface size, which uniformly magnifies every
    /// primitive the walker (or a custom [`crate::bridge::WgpuRenderable`])
    /// emits — text, buttons, panels, everything — without any widget-level
    /// code reading this value itself.
    ///
    /// `ui_scale` is clamped to [`MIN_UI_SCALE`, `MAX_UI_SCALE`]; a
    /// non-finite input (NaN, ±inf) is ignored rather than clamped, since
    /// clamping NaN is itself undefined. A value identical to the current
    /// one (post-clamp) is a no-op — mirrors [`WgpuApp::set_theme`]'s
    /// no-redundant-work contract.
    ///
    /// On the winit-owned path this re-syncs against the live window size;
    /// on the host-embedding path (no window — see [`WgpuApp::set_surface_size`])
    /// it re-syncs against the most recently reported surface size, if any.
    /// Either way the runtime tree is marked dirty and the scene is eagerly
    /// rebuilt — the same "no stale layout frame" contract a resize gets.
    ///
    /// On the winit-owned path this also marks a redraw needed (review
    /// follow-up on #179): the rebuild above only replaces `self.scene` —
    /// nothing else schedules a repaint on an idle window, so without this
    /// the previous frame would stay on screen indefinitely while
    /// hit-testing had already moved on to the new coordinates.
    /// [`WgpuApp::about_to_wait`] is what turns this into the actual
    /// `request_redraw()` call on its next tick.
    pub fn set_ui_scale(&mut self, ui_scale: f32) {
        if !ui_scale.is_finite() {
            return;
        }
        let ui_scale = ui_scale.clamp(MIN_UI_SCALE, MAX_UI_SCALE);
        if (self.ui_scale - ui_scale).abs() < f32::EPSILON {
            return;
        }
        self.ui_scale = ui_scale;
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.resync_viewport_for_ui_scale_change();
            self.redraw_requested = true;
        }
    }

    /// Borrow the underlying core Mkui (and thus the runtime AppTree),
    /// if the app was constructed via [`WgpuApp::with_app_tree`]. Tests
    /// and FFI shims call this to inspect or mutate the tree without
    /// going through the event loop.
    pub fn core(&self) -> Option<&Rc<RefCell<mkui_core::components::Mkui>>> {
        self.core.as_ref()
    }

    pub fn registry(&self) -> Option<&WgpuRendererRegistry> {
        self.registry.as_ref()
    }

    /// Override the window title used on the next `resumed` event.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn with_window_title(mut self, title: impl Into<String>) -> Self {
        self.window_title = title.into();
        self
    }

    /// Run the event loop in headless mode — build the scene once, then
    /// exit without opening a window. The `HEADLESS=1` smoke-test gate
    /// (acceptance criterion #18) routes through this path so CI can
    /// validate the bridge end-to-end without a display server.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn with_headless(mut self, headless: bool) -> Self {
        self.headless = headless;
        self
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn is_headless(&self) -> bool {
        self.headless
    }

    /// Pull the runtime tree's `is_dirty` state, rebuild the scene from
    /// it via the walker, and clear the dirty flag. No-op when the app
    /// is in scene-only mode (the legacy `with_scene` path).
    ///
    /// Exposed so headless tests can drive the walker once without
    /// running the full event loop.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn rebuild_scene_from_tree(&mut self) {
        self.try_rebuild_scene_from_tree();
    }

    /// Same work as [`WgpuApp::rebuild_scene_from_tree`], but reports
    /// whether the scene was actually replaced. `false` on a
    /// `walk_app_tree` error: the previous `scene` / `hit_tests` are left
    /// in place and the tree's dirty bit is deliberately **not** cleared,
    /// so the next call retries rather than silently going quiet.
    ///
    /// [`WgpuApp::update_frame`] needs this bool (#177 review P2) — it
    /// must not report "rebuilt" to the host when nothing was actually
    /// replaced, or a custom-renderer error would make an embedded host
    /// believe its old frame is current and stop re-uploading it, or
    /// (if it instead always re-uploaded) spin re-uploading the same
    /// stale scene every frame on a false "changed" signal.
    #[cfg(not(target_arch = "wasm32"))]
    fn try_rebuild_scene_from_tree(&mut self) -> bool {
        let Some(core) = self.core.as_ref() else {
            return false;
        };
        let Some(registry) = self.registry.as_ref() else {
            return false;
        };
        let options = WalkOptions {
            viewport: self.scene.viewport,
            theme: self.theme,
            text_system: self.text_system.as_ref(),
        };
        let output = {
            let core_ref = core.borrow();
            match walk_app_tree_cached(core_ref.tree(), registry, &options, &mut self.walk_cache) {
                Ok((out, _stats)) => out,
                Err(err) => {
                    eprintln!("mkui-wgpu: walk_app_tree error: {err}");
                    return false;
                }
            }
        };
        // Clear dirty after the walk completes so a future action firing
        // mid-walk does not get its dirty bit clobbered.
        core.borrow_mut().tree_mut().clear_dirty();
        self.scene = output.scene;
        self.hit_tests = output.hit_tests;
        true
    }

    /// Per-frame entry point for a host that ticks mkui every frame of its
    /// own render loop, rather than winit's on-demand `RedrawRequested`
    /// (#177). Unlike [`WgpuApp::rebuild_scene_from_tree`], this method
    /// owns the dirty gate itself: it is safe to call unconditionally,
    /// every frame, and on a frame where nothing changed it returns
    /// `false` having performed no tree walk and no heap allocation —
    /// `self.scene` / `self.hit_tests` from the last rebuild are left in
    /// place for the host to keep drawing.
    ///
    /// Returns `true` only when the scene was actually replaced (the host
    /// should re-tessellate / re-upload against the new `scene()`).
    /// Returns `false` when the tree is clean, when the app is in
    /// scene-only mode (`with_scene` — no tree to be dirty), and when a
    /// custom renderer's `walk_app_tree` call errors (the tree stays
    /// dirty in that case, so the next frame's call retries).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn update_frame(&mut self) -> bool {
        let Some(core) = self.core.as_ref() else {
            return false;
        };
        if !core.borrow().tree().is_dirty() {
            return false;
        }
        self.try_rebuild_scene_from_tree()
    }

    /// Push a surface-size update from a host that owns its own
    /// window/surface and drives mkui through [`WgpuApp::update_frame`]
    /// rather than letting this app own a winit window (#179, the
    /// host-embedding path that [`WgpuApp::update_frame`] already
    /// documents — #177). Before this method existed, that path had no
    /// way at all to tell `WgpuApp` its surface had resized: the viewport
    /// stayed at whatever `with_app_tree`'s construction-time default was
    /// for the life of the app.
    ///
    /// `physical_width` / `physical_height` are the host's render target
    /// size in physical pixels; `scale_factor` is the OS HiDPI scale the
    /// host's own window reports (`1.0` if the host doesn't track one).
    /// Recomputes the logical scene viewport — honouring the current
    /// [`WgpuApp::ui_scale`] the same way the winit-owned path's
    /// `Resized`/`ScaleFactorChanged` handlers do — and queues the rebuild
    /// (`WgpuApp::queue_viewport_resize`: updates `Scene::viewport` and
    /// marks the tree dirty, but does **not** walk it here) so the very
    /// next [`WgpuApp::update_frame`] call is the one that actually
    /// rebuilds and returns `true`. Rebuilding eagerly inside this method
    /// instead — as the winit-owned resize handlers correctly do — would
    /// clear the dirty bit before the host's next `update_frame()` call
    /// ever sees it, so a host that (per that method's own contract) only
    /// re-tessellates/re-uploads on a `true` return would never learn the
    /// resize was applied: no stale layout frame survives, but the host
    /// would never be told to stop drawing the previous one either.
    ///
    /// No-op when: the app owns a winit window (`WgpuApp::with_app_tree` run
    /// through [`WgpuApp`]'s own `ApplicationHandler`/`run` loop) — that
    /// path's own `Resized`/`ScaleFactorChanged` handlers already own
    /// viewport sync and must stay the single writer; or the reported size
    /// is zero (a minimized/occluded host surface) — mirrors the existing
    /// `sync_renderer_to_current_window_size` zero-size guard.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn set_surface_size(
        &mut self,
        physical_width: u32,
        physical_height: u32,
        scale_factor: f32,
    ) {
        if self.state.is_some() {
            return;
        }
        if physical_width == 0 || physical_height == 0 {
            return;
        }
        let os_scale_factor = if scale_factor.is_finite() && scale_factor > 0.0 {
            scale_factor as f64
        } else {
            1.0
        };
        self.last_surface_size = Some((physical_width, physical_height, os_scale_factor));
        let viewport = logical_viewport_from_physical_size(
            winit::dpi::PhysicalSize::new(physical_width, physical_height),
            self.effective_scale_factor_for(os_scale_factor),
        );
        self.queue_viewport_resize(viewport);
    }

    /// Update `Scene::viewport` in place and mark the tree dirty, without
    /// walking it (#179). The host-embedding counterpart to
    /// `WgpuApp::resize_scene_viewport`'s eager rebuild: a host polling
    /// [`WgpuApp::update_frame`]'s return value must be the one to trigger
    /// the actual walk, or it never observes the `true` that tells it to
    /// re-upload. See [`WgpuApp::set_surface_size`]'s doc for why eager
    /// rebuild is wrong on this path.
    ///
    /// A no-op on the raw-scene escape hatch (`self.core` is `None`) beyond
    /// the viewport update itself — mirrors `resize_scene_viewport`'s
    /// raw-scene branch; there is no tree to mark dirty.
    #[cfg(not(target_arch = "wasm32"))]
    fn queue_viewport_resize(&mut self, new_viewport: Size) {
        self.scene.viewport = new_viewport;
        if let Some(core) = self.core.as_ref() {
            core.borrow_mut().tree_mut().mark_dirty();
        }
    }

    /// Fold [`WgpuApp::ui_scale`] into an OS-reported `scale_factor` so every
    /// physical→logical conversion in this module (viewport AND cursor
    /// position) divides by the same combined factor (#179, ADR 0006
    /// §"UI-scale setting"). A mismatch between the two — e.g. the viewport
    /// honouring `ui_scale` while hit-testing didn't — would silently
    /// misalign clicks the moment `ui_scale != 1.0`.
    #[cfg(not(target_arch = "wasm32"))]
    fn effective_scale_factor_for(&self, os_scale_factor: f64) -> f64 {
        os_scale_factor * self.ui_scale as f64
    }

    /// The combined factor a latched `MouseInput` cursor position must
    /// divide by to land in the same logical-pixel space `hit_tests` was
    /// produced against (#179). Reads the live window's OS `scale_factor`
    /// when one exists (`1.0` on the scene-only/no-window path), then
    /// folds in [`WgpuApp::ui_scale`] via [`WgpuApp::effective_scale_factor_for`]
    /// — the same helper the viewport computation uses, so the two can
    /// never silently disagree.
    #[cfg(not(target_arch = "wasm32"))]
    fn cursor_scale_factor(&self) -> f64 {
        let os_scale_factor = self
            .state
            .as_ref()
            .map(|s| s.window.scale_factor())
            .unwrap_or(1.0);
        self.effective_scale_factor_for(os_scale_factor)
    }

    /// Re-sync the viewport after [`WgpuApp::set_ui_scale`] changes the
    /// scale alone (no accompanying resize event), or right after
    /// `resumed()` creates the window (to pick up a `ui_scale` the caller
    /// set before `run()` — see ADR 0006 §"UI-scale setting"). Re-derives
    /// the physical size from whichever path is active: the live window on
    /// the winit-owned path (eager rebuild — that path isn't gated by
    /// `update_frame`'s return value), or the most recent
    /// [`WgpuApp::set_surface_size`] report on the host-embedding path
    /// (queued, not eager — see that method's doc for why). A no-op if
    /// neither has a size yet (construction-time default viewport stands
    /// until the first resize).
    #[cfg(not(target_arch = "wasm32"))]
    fn resync_viewport_for_ui_scale_change(&mut self) {
        if let Some(state) = self.state.as_ref() {
            let size = state.window.inner_size();
            if size.width == 0 || size.height == 0 {
                return;
            }
            // Deliberately does NOT call `arm_resize_redraw_pump()`: that
            // pump exists to bridge an actual Metal swapchain transition
            // during a real `Renderer::resize` (#99/#101) — this branch
            // never touches the renderer/surface config, only the logical
            // viewport, so there is no transition to bridge. Arming it
            // here (an earlier revision of #179 did) forced ~60 redraws
            // and opened the macOS `CursorMoved` bridge window on every
            // `resumed()` call, since this method also runs once at
            // startup to pick up a `ui_scale` set before `run()`.
            let viewport = logical_viewport_from_physical_size(
                size,
                self.effective_scale_factor_for(state.window.scale_factor()),
            );
            self.resize_scene_viewport(viewport);
            // Scheduling the actual repaint is the caller's job (#179
            // review follow-up): `set_ui_scale` sets `redraw_requested`
            // after calling this method, and `about_to_wait` is what
            // turns that into a real `request_redraw()` call. `resumed()`
            // (this method's other caller) already requests one
            // unconditionally right after, so neither caller needs this
            // method itself to touch the window.
        } else if let Some((width, height, os_scale_factor)) = self.last_surface_size {
            let viewport = logical_viewport_from_physical_size(
                winit::dpi::PhysicalSize::new(width, height),
                self.effective_scale_factor_for(os_scale_factor),
            );
            self.queue_viewport_resize(viewport);
        }
    }

    /// Update the held scene for a new viewport on `WindowEvent::Resized`,
    /// honouring the per-path resize contract (ADR 0006 §"Resize behaviour
    /// contract"):
    ///
    /// - **Declarative / AppTree path** (`self.core` is `Some`): the scene
    ///   is a per-frame projection of the runtime tree, so a resize wipes
    ///   it and eagerly rebuilds from the (now-dirty) tree against the new
    ///   viewport.
    /// - **Raw-scene escape hatch** (`with_scene`, `self.core` is `None`):
    ///   the user owns the scene and its contract is "I gave you primitives;
    ///   render them across resizes." Replacing it with a fresh empty scene
    ///   wiped the user's primitives before first paint (#93). We now only
    ///   update the viewport in place so the primitives survive.
    #[cfg(not(target_arch = "wasm32"))]
    fn resize_scene_viewport(&mut self, new_viewport: Size) {
        if self.core.is_some() {
            self.scene = Scene::new(new_viewport);
            if let Some(core) = self.core.as_ref() {
                core.borrow_mut().tree_mut().mark_dirty();
            }
            self.rebuild_scene_from_tree();
        } else {
            self.scene.viewport = new_viewport;
        }
    }

    /// Advance first-paint retry state for a completed render outcome and
    /// report whether the caller should schedule another `request_redraw()`.
    ///
    /// Background (#93 round-N+1): `resumed()` schedules exactly one redraw
    /// after creating the window. wgpu returns [`RenderOutcome::Skipped`]
    /// when the surface isn't ready yet (Timeout / Occluded / Validation),
    /// and the event loop has no other redraw trigger on an idle UI — so a
    /// `Skipped` first frame left the window blank-gray until the user
    /// resized or interacted. The fix reschedules on `Skipped` **only while
    /// first paint is pending** (capped to avoid an occluded-window spin):
    ///
    /// - `Drawn`: first paint succeeded — clear the flag; also consume one
    ///   frame of the resize pump budget (#99: the pump decays on presented
    ///   frames, not `about_to_wait` ticks). Idle afterwards.
    /// - `Skipped`: retry while `first_paint_pending` and retries remain;
    ///   once the flag is cleared, `Skipped` is a no-op so idle frames idle.
    ///   `Skipped` never touches the resize pump — skipped frames don't burn
    ///   its budget (StoneSketch-style).
    /// - `NeedsReconfigure`: surface was reconfigured by the caller; drive
    ///   another frame and keep first paint pending.
    #[cfg(not(target_arch = "wasm32"))]
    fn handle_render_outcome_for_redraw(&mut self, outcome: RenderOutcome) -> bool {
        match outcome {
            RenderOutcome::Drawn => {
                self.first_paint_pending = false;
                self.consume_resize_redraw_frame();
                false
            }
            RenderOutcome::Skipped => {
                if self.first_paint_pending && self.first_paint_skip_retries > 0 {
                    self.first_paint_skip_retries -= 1;
                    true
                } else {
                    false
                }
            }
            RenderOutcome::NeedsReconfigure => true,
        }
    }

    /// Arm the resize-active redraw pump (#99). Called from the `Resized`
    /// and `ScaleFactorChanged` handlers. Subsequent arms reset the counter
    /// to the cap rather than stacking (Codex Q2) — a fresh resize event
    /// renews the recovery window, it doesn't extend it unbounded.
    ///
    /// Also stamps `last_resize_frame` with the current `frame_counter` (#101)
    /// to (re)open the active-resize window that gates the `CursorMoved` M2
    /// bridge. The two pieces of state are armed together but decay
    /// independently: the pump drains on presented `Drawn` frames, while the
    /// active-resize window expires on a frame-count timeout.
    #[cfg(not(target_arch = "wasm32"))]
    fn arm_resize_redraw_pump(&mut self) {
        self.resize_redraw_pending = RESIZE_REDRAW_PUMP_TICKS;
        self.last_resize_frame = Some(self.frame_counter);
    }

    /// True while inside the active-resize window (#101): a live-resize
    /// gesture armed `last_resize_frame` within the last
    /// [`RESIZE_ACTIVE_WINDOW_FRAMES`] frames. Gates the `CursorMoved` M2
    /// redraw bridge.
    ///
    /// Deliberately independent of `resize_redraw_pending`: encoding this guard
    /// as `resize_redraw_pending > 0` is circular (a drained pump reads as "not
    /// resizing" exactly when the residual fast-shrink vibration needs
    /// cursor-driven redraws — Codex 2026-06-09 #101 review P1#2). The
    /// frame-counter timeout closes the window even if the pump is still armed,
    /// and keeps it open even if the pump has drained — they track orthogonal
    /// questions ("is the gesture recent?" vs "does the pump have budget?").
    #[cfg(not(target_arch = "wasm32"))]
    fn in_active_resize_window(&self) -> bool {
        match self.last_resize_frame {
            None => false,
            Some(armed_at) => {
                self.frame_counter.saturating_sub(armed_at) < RESIZE_ACTIVE_WINDOW_FRAMES
            }
        }
    }

    /// Whether a `CursorMoved` event should drive the M2 redraw bridge this
    /// tick (#101). The active-resize-window state machine
    /// ([`WgpuApp::in_active_resize_window`]) is **shared infra** — armed,
    /// decayed, and unit-tested identically on every native target — but the
    /// cursor-driven redraw it gates is **macOS-only**: only macOS exhibits
    /// the fast-shrink residual vibration #101 fixes, and the brief scopes the
    /// fix to macOS ("macos fast-resize residual vibration"). On Windows/Linux
    /// the bridge folds to a constant `false` at compile time (`#[cfg]`, not a
    /// runtime `cfg!` branch), so ordinary cursor motion during a resize stays
    /// quiescent there — no behavior change on those platforms.
    ///
    /// The shared predicate is still evaluated on every target so the state
    /// machine and its backing fields stay live (and identically exercised by
    /// the cross-platform unit tests); only the *bridge action* gates on the
    /// platform.
    #[cfg(not(target_arch = "wasm32"))]
    fn cursor_moved_should_request_redraw(&self) -> bool {
        let in_active_resize = self.in_active_resize_window();
        #[cfg(target_os = "macos")]
        {
            in_active_resize
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = in_active_resize;
            false
        }
    }

    /// True while the resize pump is armed — i.e. `about_to_wait` should
    /// drive another redraw this tick. A pure read: the pump is *not*
    /// drained by being asked whether to redraw (StoneSketch-style — the
    /// budget is measured in presented frames, not event-loop ticks).
    #[cfg(not(target_arch = "wasm32"))]
    fn resize_redraw_pump_armed(&self) -> bool {
        self.resize_redraw_pending > 0
    }

    /// Consume one frame of the resize pump budget. Called only on a
    /// successful `RenderOutcome::Drawn` (operator's 2026-06-09 StoneSketch
    /// deep-read): the active-redraw window measures *presented* frames, so
    /// `Skipped` frames under GPU pressure during fast resize do not burn the
    /// budget — otherwise the bridge exhausts before the gesture ends and the
    /// jerk persists. The saturating check prevents underflow at `0`.
    #[cfg(not(target_arch = "wasm32"))]
    fn consume_resize_redraw_frame(&mut self) {
        if self.resize_redraw_pending > 0 {
            self.resize_redraw_pending -= 1;
        }
    }

    /// Last chance sync before drawing. During fast macOS live-resize the
    /// platform layer can advance before the matching `Resized` event has
    /// drained through winit. If we acquire/present against the stale surface
    /// config in that window, CoreAnimation may scale the previous drawable
    /// for a frame. Query the authoritative window size at redraw time and
    /// bring both renderer config and logical scene viewport forward before
    /// we build vertices.
    #[cfg(not(target_arch = "wasm32"))]
    fn sync_renderer_to_current_window_size(&mut self) {
        let ui_scale = self.ui_scale;
        let new_viewport = {
            let Some(state) = self.state.as_mut() else {
                return;
            };
            let size = state.window.inner_size();
            if size.width == 0 || size.height == 0 {
                return;
            }
            if state.renderer.size() == (size.width, size.height) {
                return;
            }
            state.renderer.resize(size.width, size.height);
            logical_viewport_from_physical_size(size, state.window.scale_factor() * ui_scale as f64)
        };
        self.resize_scene_viewport(new_viewport);
        self.arm_resize_redraw_pump();
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn window_attributes(&self) -> WindowAttributes {
        let viewport = self.scene.viewport;
        Window::default_attributes()
            .with_title(self.window_title.clone())
            .with_inner_size(winit::dpi::LogicalSize::new(
                viewport.width as f64,
                viewport.height as f64,
            ))
    }

    /// Fire the action attached to `click_hit` through the tree's
    /// `ActionRegistry`. Returns true if a redraw signal was raised.
    /// Caller must have already resolved `click_hit` via the press-to-
    /// arm state machine (see [`crate::pointer::PointerState::on_release`]).
    #[cfg(not(target_arch = "wasm32"))]
    fn fire_action(&mut self, click_hit: crate::pointer::ClickHit) -> bool {
        let Some(action_id) = click_hit.action else {
            return false;
        };
        let Some(core) = self.core.as_ref() else {
            return false;
        };
        // Borrow scoping: `fire` only needs `&self`, so we drop the
        // immutable borrow before re-borrowing mutably to propagate the
        // dirty bit. Holding both at once is the round-7 anti-pattern.
        let mut ctx = core.borrow().tree().actions().fire(action_id);
        if ctx.is_dirty() {
            core.borrow_mut().tree_mut().mark_dirty();
        }
        ctx.drain_emitted()
            .iter()
            .any(|s| matches!(s, RuntimeSignal::RequestRedraw))
    }
}

impl std::fmt::Debug for WgpuApp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut builder = f.debug_struct("WgpuApp");
        builder
            .field("scene", &self.scene)
            .field("text_system", &"Arc<dyn TextSystem>")
            .field("has_core", &self.core.is_some())
            .field("has_registry", &self.registry.is_some());
        #[cfg(not(target_arch = "wasm32"))]
        builder
            .field("state", &self.state)
            .field("window_title", &self.window_title)
            .field("headless", &self.headless)
            .field("hit_tests", &self.hit_tests.len());
        builder.finish()
    }
}

/// Convert a physical-pixel window size into the logical-pixel viewport
/// that `Scene::viewport` carries (ADR 0006 §"Viewport units contract").
///
/// wgpu configures its surface in physical pixels, but scene primitives are
/// authored in logical pixels (matching web's CSS-pixel and console's
/// character-grid conventions), so the resize/scale-change boundary divides
/// the physical size by `scale_factor`. Free function, not a method, per the
/// helper-not-API convention (#96): it has no `self` dependency and keeps the
/// `physical / scale_factor` conversion in one testable place. Fractional
/// scale factors (e.g. 1.5×) are preserved as f32 — we do not round to
/// integer logical pixels (Codex Q8).
///
/// `scale_factor` is whatever divisor the caller wants folded into the
/// viewport — every call site in this module now passes the OS
/// `scale_factor` already combined with [`WgpuApp::ui_scale`] (#179, via
/// the private `WgpuApp::effective_scale_factor_for` helper) rather than
/// the raw OS value, so this function itself stays agnostic to where the
/// factor came from.
#[cfg(not(target_arch = "wasm32"))]
fn logical_viewport_from_physical_size(
    size: winit::dpi::PhysicalSize<u32>,
    scale_factor: f64,
) -> Size {
    let scale = (scale_factor as f32).max(f32::EPSILON);
    Size::new(size.width as f32 / scale, size.height as f32 / scale)
}

#[cfg(not(target_arch = "wasm32"))]
impl ApplicationHandler for WgpuApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        // Headless mode: walk once for the smoke gate, then exit without
        // ever creating a window or wgpu surface. Lets CI validate the
        // walker + registry wiring end-to-end on machines without a
        // display server (acceptance criterion #18).
        if self.headless {
            self.rebuild_scene_from_tree();
            event_loop.exit();
            return;
        }

        // `resumed` can fire more than once on Android-style lifecycles;
        // bail out if we already have a window so we don't drop the
        // current renderer.
        if self.state.is_some() {
            return;
        }
        let window = match event_loop.create_window(self.window_attributes()) {
            Ok(window) => Arc::new(window),
            Err(error) => {
                eprintln!("mkui-wgpu: failed to create window: {error}");
                event_loop.exit();
                return;
            }
        };
        // pollster: deliberate sync init — wgpu adapter/device resolution is a
        // one-shot at window creation, not a per-frame cost, so blocking the
        // resumed-event handler here is cheaper than threading an async runtime
        // through the winit `ApplicationHandler` (ADR 0004).
        match pollster::block_on(Renderer::new(window.clone())) {
            Ok(renderer) => {
                self.state = Some(WgpuAppState { window, renderer });
                if self.core.is_some() {
                    // Re-derive the viewport from the window's real
                    // inner_size()/scale_factor() rather than trusting
                    // whatever `self.scene.viewport` held before the window
                    // existed — the OS may not have honoured
                    // `window_attributes()`'s requested size exactly, and a
                    // caller may have set `ui_scale` before calling `run()`
                    // (#179). Without this, the first frame lays out at a
                    // stale/unscaled viewport while `MouseInput` already
                    // hit-tests against the correct one, misaligning clicks
                    // until the next `Resized` event happens to arrive.
                    self.resync_viewport_for_ui_scale_change();
                }
                if let Some(state) = self.state.as_ref() {
                    state.window.request_redraw();
                }
            }
            Err(error) => {
                eprintln!("mkui-wgpu: failed to initialize renderer: {error}");
                event_loop.exit();
            }
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        let window_matches = self
            .state
            .as_ref()
            .is_some_and(|state| state.window.id() == window_id);
        if !window_matches {
            return;
        }
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                let ui_scale = self.ui_scale;
                let new_viewport = {
                    let Some(state) = self.state.as_mut() else {
                        return;
                    };
                    state.renderer.resize(size.width, size.height);
                    logical_viewport_from_physical_size(
                        size,
                        state.window.scale_factor() * ui_scale as f64,
                    )
                };
                self.resize_scene_viewport(new_viewport);
                self.arm_resize_redraw_pump();
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                // A window dragged between displays of different DPIs keeps
                // its physical size but changes scale_factor; recompute the
                // logical viewport so primitives stay correctly proportioned
                // (Codex Q3). We deliberately do not touch `inner_size_writer`
                // — forcing a window size is a UX decision out of scope here
                // (Codex Q9 anti-pattern).
                let ui_scale = self.ui_scale;
                let new_viewport = {
                    let Some(state) = self.state.as_mut() else {
                        return;
                    };
                    let size = state.window.inner_size();
                    state.renderer.resize(size.width, size.height);
                    logical_viewport_from_physical_size(size, scale_factor * ui_scale as f64)
                };
                self.resize_scene_viewport(new_viewport);
                self.arm_resize_redraw_pump();
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.pointer.update_cursor(position);
                // M2 (#101): during the active-resize window, cursor motion
                // directly schedules a redraw — INDEPENDENT of the resize
                // pump's counter (Codex 2026-06-09 #101 review P1#2 + P2#2).
                // This complements the pump's `Resized`-arm trigger by giving
                // the redraw cadence a cursor-driven trigger, smoothing the
                // residual fast-shrink vibration toward cursor cadence rather
                // than `Resized` cadence. It is NOT M1 (re-arming the pump):
                // cap-exhaustion was ruled out, so re-arming would not help and
                // the circular `resize_redraw_pending > 0` guard is avoided.
                // The bridge is **macOS-only** (see
                // `cursor_moved_should_request_redraw`): only macOS exhibits
                // the fast-shrink residual, so Windows/Linux cursor motion
                // stays quiescent. The active-resize-window timeout keeps even
                // macOS quiescent once the gesture stops.
                if self.cursor_moved_should_request_redraw() {
                    if let Some(state) = self.state.as_ref() {
                        state.window.request_redraw();
                    }
                }
            }
            WindowEvent::CursorLeft { .. } => {
                // Mouse left the window: cancel any armed press without
                // firing (Codex round-10 Q4 — drag-cancel affordance).
                self.pointer.cancel();
            }
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        physical_key: PhysicalKey::Code(KeyCode::Escape),
                        state: ElementState::Pressed,
                        ..
                    },
                ..
            } => {
                // User-driven cancel: Escape clears the armed slot
                // without firing.
                self.pointer.cancel();
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let Some(state) = left_button_state(button, state) else {
                    return;
                };
                let Some(cursor) = self.pointer.cursor_logical(self.cursor_scale_factor()) else {
                    return;
                };
                match state {
                    ElementState::Pressed => {
                        // Press-to-arm: record the topmost-hit node;
                        // never fire on press (Codex round-10 Q4).
                        self.pointer.on_press(&self.hit_tests, cursor);
                    }
                    ElementState::Released => {
                        let Some(click) = self.pointer.on_release(&self.hit_tests, cursor) else {
                            // Release on different node / empty space /
                            // after cancel — armed slot already cleared
                            // inside `on_release`. Nothing to fire.
                            return;
                        };
                        if self.fire_action(click) {
                            // Action requested a redraw — rebuild from
                            // the (now-dirty) tree and ask winit for a
                            // redraw. The `request_redraw` call is made
                            // HERE in the event loop, NOT from inside
                            // the action closure (Sprint 4 anti-pattern
                            // guard).
                            self.rebuild_scene_from_tree();
                            if let Some(state) = self.state.as_ref() {
                                state.window.request_redraw();
                            }
                        }
                    }
                }
            }
            WindowEvent::RedrawRequested => {
                self.sync_renderer_to_current_window_size();
                // If the tree dirtied between events, rebuild before painting.
                let needs_rebuild = self
                    .core
                    .as_ref()
                    .map(|c| c.borrow().tree().is_dirty())
                    .unwrap_or(false);
                if needs_rebuild {
                    self.rebuild_scene_from_tree();
                }
                // Render inside a scoped borrow, reconfiguring in place on
                // `NeedsReconfigure`, then drop the borrow so the first-paint
                // retry bookkeeping can take `&mut self`.
                let outcome = {
                    let Some(state) = self.state.as_mut() else {
                        return;
                    };
                    match state.renderer.render(&self.scene, &*self.text_system) {
                        Ok(RenderOutcome::NeedsReconfigure) => {
                            let (w, h) = state.renderer.size();
                            state.renderer.resize(w, h);
                            RenderOutcome::NeedsReconfigure
                        }
                        Ok(outcome) => outcome,
                        Err(error) => {
                            eprintln!("mkui-wgpu: render error: {error}");
                            event_loop.exit();
                            return;
                        }
                    }
                };
                // Schedule another redraw when the outcome warrants it — a
                // reconfigure, or a first-paint `Skipped` retry (#93).
                if self.handle_render_outcome_for_redraw(outcome) {
                    if let Some(state) = self.state.as_ref() {
                        state.window.request_redraw();
                    }
                }
            }
            _ => {}
        }
    }

    /// Resize-active redraw pump (#99). macOS fires `Resized` continuously
    /// during a live-resize gesture; between events the OS may present a
    /// frame at the new layer size before the next rendered frame catches
    /// up, producing a visible scale-snap jerk on shrinking gestures. While
    /// the pump is armed (set on `Resized` / `ScaleFactorChanged`), drive a
    /// burst of redraws so frames keep pace with the Metal swapchain
    /// transition, then let the pump decay so idle windows stay quiescent.
    /// The pump decays on *presented* frames (consumed in
    /// `handle_render_outcome_for_redraw` on `Drawn`), not on these ticks —
    /// see ADR 0006 §"Resize-active redraw pump".
    ///
    /// This tick also advances `frame_counter` (#101), the monotonic clock the
    /// active-resize-window guard (`WgpuApp::in_active_resize_window`) reads
    /// to expire the `CursorMoved` M2 bridge a fixed number of frames after the
    /// last `Resized` — keeping that bridge live across a fast gesture without
    /// redrawing on idle pointer motion afterward.
    ///
    /// Also the sole consumer of `redraw_requested` (#179 review
    /// follow-up): [`WgpuApp::set_ui_scale`] sets it on the winit-owned
    /// path instead of arming the full resize pump (a `ui_scale` change
    /// doesn't touch the renderer/surface config, so it needs one redraw,
    /// not a 60-tick swapchain-transition bridge). Cleared unconditionally
    /// every tick — a one-shot ask, not a standing need.
    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        self.on_about_to_wait_tick();
    }
}

impl WgpuApp {
    /// `about_to_wait`'s actual logic, factored out so it's callable from
    /// a unit test without a live `ActiveEventLoop` (which can't be
    /// constructed outside a running winit event loop) — `about_to_wait`
    /// itself never reads its `_event_loop` parameter, so delegating to
    /// this costs nothing. See [`WgpuApp::redraw_requested`]'s doc for why
    /// this tick also doubles as that flag's sole consumer.
    #[cfg(not(target_arch = "wasm32"))]
    fn on_about_to_wait_tick(&mut self) {
        self.frame_counter = self.frame_counter.saturating_add(1);
        if self.resize_redraw_pump_armed() || self.redraw_requested {
            if let Some(state) = self.state.as_ref() {
                state.window.request_redraw();
            }
        }
        self.redraw_requested = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Size;

    #[test]
    fn new_holds_initial_scene() {
        let scene = Scene::new(Size::new(640.0, 480.0));
        let app = WgpuApp::new(scene);
        assert_eq!(app.scene().viewport, Size::new(640.0, 480.0));
        assert!(app.core().is_none(), "scene-only constructor has no core");
    }

    #[test]
    fn set_scene_replaces_held_scene() {
        let mut app = WgpuApp::new(Scene::new(Size::new(100.0, 100.0)));
        app.set_scene(Scene::new(Size::new(200.0, 200.0)));
        assert_eq!(app.scene().viewport, Size::new(200.0, 200.0));
    }

    #[test]
    fn with_app_tree_attaches_core_and_registry() {
        let core = mkui_core::components::Mkui::new();
        let registry = WgpuRendererRegistry::with_defaults();
        let app = WgpuApp::with_app_tree(core, registry);
        assert!(app.core().is_some());
        assert!(app.registry().is_some());
    }

    #[test]
    fn with_app_tree_and_text_system_retains_supplied_system() {
        use mkui_text::CompositeTextSystem;
        // #66: a declarative app handed a custom/composite text system must keep
        // it — never silently swap in the bitmap default.
        let core = mkui_core::components::Mkui::new();
        let registry = WgpuRendererRegistry::with_defaults();
        let ts: Arc<dyn TextSystem> = Arc::new(CompositeTextSystem::new());
        let app = WgpuApp::with_app_tree_and_text_system(core, registry, Arc::clone(&ts));
        assert!(app.core().is_some(), "still a declarative-path app");
        assert!(
            Arc::ptr_eq(app.text_system(), &ts),
            "the supplied text system is retained, not replaced by the bitmap default"
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn logical_viewport_handles_integer_scale_factor() {
        // Retina 2× display: 1600×1200 physical → 800×600 logical (#97).
        let viewport =
            logical_viewport_from_physical_size(winit::dpi::PhysicalSize::new(1600, 1200), 2.0);
        assert_eq!(viewport.width, 800.0);
        assert_eq!(viewport.height, 600.0);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn logical_viewport_handles_fractional_scale_factor() {
        // Fractional scale (1.5×) is preserved, not rounded (Codex Q8):
        // 1200×900 physical → 800×600 logical.
        let viewport =
            logical_viewport_from_physical_size(winit::dpi::PhysicalSize::new(1200, 900), 1.5);
        assert_eq!(viewport.width, 800.0);
        assert_eq!(viewport.height, 600.0);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn raw_scene_survives_resize() {
        use crate::types::{Color, CornerRadii, Point, Primitive, Quad, Rect};

        // with_scene mode (core is None): the user's primitives must
        // outlive a resize — only the viewport updates in place (#93).
        let mut scene = Scene::new(Size::new(800.0, 600.0));
        scene.push(Primitive::Quad(Quad {
            rect: Rect::new(Point::new(200.0, 150.0), Size::new(400.0, 300.0)),
            fill: Color::rgba(0.42, 0.66, 0.84, 1.0),
            corner_radii: CornerRadii::all(0.0),
            stroke: None,
        }));
        let mut app = WgpuApp::new(scene);
        app.resize_scene_viewport(Size::new(1024.0, 768.0));

        assert_eq!(
            app.scene().primitives.len(),
            1,
            "raw-scene primitives must survive resize"
        );
        assert_eq!(
            app.scene().viewport,
            Size::new(1024.0, 768.0),
            "viewport must update in place on resize"
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn declarative_resize_rebuilds_from_tree() {
        use mkui_core::components::{Mkui as CoreMkui, Text};
        use mkui_runtime::TextVariant;

        // AppTree mode (core is Some): a resize wipes the projected scene
        // and eagerly rebuilds it from the tree against the new viewport.
        let core = CoreMkui::new().child(Text::new("hi").variant(TextVariant::Heading1));
        let registry = WgpuRendererRegistry::with_defaults();
        let mut app = WgpuApp::with_app_tree(core, registry);
        app.resize_scene_viewport(Size::new(1024.0, 768.0));

        let has_text = app
            .scene()
            .primitives
            .iter()
            .any(|p| matches!(p, crate::types::Primitive::Text(_)));
        assert!(has_text, "resize must rebuild the tree-projected scene");
        assert_eq!(
            app.scene().viewport,
            Size::new(1024.0, 768.0),
            "rebuilt scene must adopt the new viewport"
        );
    }

    // ---- #179: resolution, HiDPI, live resize, UI-scale setting ----

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn logical_viewport_round_trips_across_the_acceptance_size_matrix() {
        // The #179 acceptance criteria name a size list (handheld,
        // small-desktop, wide-desktop) and 1x/2x/3x HiDPI scale factors.
        // The ~390x844 phone-portrait size is deliberately excluded — the
        // issue itself ties it to the WASM/mobile gate tracked separately
        // as #180, out of scope here.
        let logical_sizes = [(1280.0_f32, 800.0_f32), (1280.0, 720.0), (3440.0, 1440.0)];
        let scale_factors = [1.0_f64, 2.0, 3.0];
        for (width, height) in logical_sizes {
            for scale in scale_factors {
                let physical = winit::dpi::PhysicalSize::new(
                    (width as f64 * scale).round() as u32,
                    (height as f64 * scale).round() as u32,
                );
                let viewport = logical_viewport_from_physical_size(physical, scale);
                assert!(
                    (viewport.width - width).abs() < 0.01,
                    "width mismatch at {width}x{height} @ {scale}x: got {}",
                    viewport.width
                );
                assert!(
                    (viewport.height - height).abs() < 0.01,
                    "height mismatch at {width}x{height} @ {scale}x: got {}",
                    viewport.height
                );
            }
        }
    }

    #[test]
    fn ui_scale_defaults_to_one() {
        let app = WgpuApp::new(Scene::new(Size::new(100.0, 100.0)));
        assert_eq!(app.ui_scale(), 1.0);
    }

    #[test]
    fn set_ui_scale_clamps_and_rejects_non_finite_input() {
        let mut app = WgpuApp::new(Scene::new(Size::new(100.0, 100.0)));
        app.set_ui_scale(f32::NAN);
        assert_eq!(app.ui_scale(), 1.0, "a non-finite value must be ignored");
        app.set_ui_scale(100.0);
        assert_eq!(
            app.ui_scale(),
            MAX_UI_SCALE,
            "must clamp to the upper bound"
        );
        app.set_ui_scale(0.0);
        assert_eq!(
            app.ui_scale(),
            MIN_UI_SCALE,
            "must clamp to the lower bound"
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn set_ui_scale_is_a_no_op_for_the_current_value() {
        let core = mkui_core::components::Mkui::new();
        let registry = WgpuRendererRegistry::with_defaults();
        let mut app = WgpuApp::with_app_tree(core, registry);
        app.set_surface_size(1600, 1200, 1.0);
        assert!(app.update_frame(), "the surface-size push must be observed");
        assert!(
            !app.update_frame(),
            "the tree is clean immediately after a rebuild"
        );

        app.set_ui_scale(1.0);

        assert!(
            !app.update_frame(),
            "re-setting the already-active ui_scale must not force a redundant rebuild"
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn set_surface_size_rebuilds_the_declarative_scene_with_no_stale_frame() {
        // #179 acceptance #2, for the host-embedding path specifically:
        // before this method existed, a host driving `WgpuApp` through
        // `update_frame()` (no winit window of its own) had no way at all
        // to tell it about a resize.
        //
        // Deliberately asserts through `update_frame()`'s return value, not
        // by peeking at `scene()` directly: that return value is the only
        // signal #177's contract tells a host to act on, and an earlier
        // revision of this method rebuilt eagerly inside `set_surface_size`
        // itself — which cleared the dirty bit before the host's next
        // `update_frame()` call could ever observe it.
        use mkui_core::components::{Mkui as CoreMkui, Text};
        use mkui_runtime::TextVariant;

        let core = CoreMkui::new().child(Text::new("hi").variant(TextVariant::Heading1));
        let registry = WgpuRendererRegistry::with_defaults();
        let mut app = WgpuApp::with_app_tree(core, registry);

        app.set_surface_size(1600, 1200, 2.0);
        assert!(
            app.update_frame(),
            "update_frame() must report the rebuild the host is supposed to react to"
        );

        assert_eq!(
            app.scene().viewport,
            Size::new(800.0, 600.0),
            "surface size / scale_factor must produce the logical viewport"
        );
        let has_text = app
            .scene()
            .primitives
            .iter()
            .any(|p| matches!(p, crate::types::Primitive::Text(_)));
        assert!(
            has_text,
            "the scene already reflects the tree at the new surface size — no stale frame"
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn set_surface_size_ignores_a_zero_sized_update() {
        let core = mkui_core::components::Mkui::new();
        let registry = WgpuRendererRegistry::with_defaults();
        let mut app = WgpuApp::with_app_tree(core, registry);
        let before = app.scene().viewport;

        app.set_surface_size(0, 600, 1.0);

        assert_eq!(
            app.scene().viewport,
            before,
            "a zero-sized (minimized/occluded) surface report must be ignored"
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn set_ui_scale_shrinks_the_logical_viewport_for_an_embedded_host() {
        // The core of #179's UI-scale feature: it folds into the same
        // logical viewport the OS HiDPI scale factor does, so every
        // primitive the walker (and any custom WgpuRenderable) emits
        // scales uniformly on screen — not just text, and with no
        // widget-level code change anywhere.
        let core = mkui_core::components::Mkui::new();
        let registry = WgpuRendererRegistry::with_defaults();
        let mut app = WgpuApp::with_app_tree(core, registry);
        app.set_surface_size(1600, 1200, 1.0);
        assert!(app.update_frame());
        assert_eq!(app.scene().viewport, Size::new(1600.0, 1200.0));

        app.set_ui_scale(2.0);

        assert!(
            app.update_frame(),
            "a ui_scale change must be observable through update_frame(), \
             not just by peeking at scene() directly"
        );
        assert_eq!(
            app.scene().viewport,
            Size::new(800.0, 600.0),
            "doubling ui_scale must halve the logical viewport, magnifying \
             every primitive 2x on screen for a fixed physical surface size"
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn set_ui_scale_without_a_known_surface_size_is_a_no_op_on_the_viewport() {
        // Neither a winit window nor a prior `set_surface_size` call exists
        // yet — there is nothing to re-sync against, so the
        // construction-time default viewport must stand untouched.
        let core = mkui_core::components::Mkui::new();
        let registry = WgpuRendererRegistry::with_defaults();
        let mut app = WgpuApp::with_app_tree(core, registry);
        let before = app.scene().viewport;

        app.set_ui_scale(2.0);

        assert_eq!(app.scene().viewport, before);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn set_ui_scale_marks_a_redraw_needed() {
        // Review follow-up (#179): `set_ui_scale` rebuilds `self.scene`
        // but, before this flag existed, nothing scheduled a repaint — on
        // an idle window the previous frame stayed on screen indefinitely
        // while hit-testing had already moved on to the new coordinates.
        // `about_to_wait` is the only consumer of this flag (it has no
        // `self` dependency other than plain data, so it's assertable
        // here without a live winit window — see `arm_resize_redraw_pump`'s
        // own tests for the same pattern on the sibling resize-jerk pump).
        let mut app = WgpuApp::new(Scene::new(Size::new(800.0, 600.0)));
        assert!(
            !app.redraw_requested,
            "must start false — nothing has changed yet"
        );

        app.set_ui_scale(2.0);

        assert!(
            app.redraw_requested,
            "a real ui_scale change must mark a redraw needed"
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn set_ui_scale_no_op_does_not_mark_a_redraw_needed() {
        let mut app = WgpuApp::new(Scene::new(Size::new(800.0, 600.0)));

        app.set_ui_scale(1.0); // already the default — a no-op

        assert!(
            !app.redraw_requested,
            "re-setting the already-active ui_scale must not spuriously \
             request a redraw"
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn about_to_wait_consumes_the_redraw_requested_flag() {
        // Calls `on_about_to_wait_tick` directly — the real `about_to_wait`
        // (the `ApplicationHandler` trait method) needs a live
        // `&ActiveEventLoop`, which can't be constructed outside a
        // running winit event loop, but it never reads that parameter;
        // `on_about_to_wait_tick` is the same logic with that
        // winit-specific plumbing factored out. `state` is `None` in this
        // construction, so the actual `request_redraw()` call is skipped,
        // same as every other test in this module — only the flag's
        // one-shot consumption and the frame counter are asserted.
        let mut app = WgpuApp::new(Scene::new(Size::new(800.0, 600.0)));
        app.redraw_requested = true;
        let before_frame_counter = app.frame_counter;

        app.on_about_to_wait_tick();

        assert!(
            !app.redraw_requested,
            "the flag must be a one-shot ask, cleared after being observed"
        );
        assert_eq!(
            app.frame_counter,
            before_frame_counter + 1,
            "about_to_wait must still advance the frame counter (#101)"
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn set_ui_scale_keeps_hit_testing_aligned_with_the_scaled_layout() {
        // #179: the viewport divides by `scale_factor * ui_scale`, so
        // hit-testing must divide a latched cursor position by that same
        // combined factor (`WgpuApp::cursor_scale_factor`, the exact
        // method `WindowEvent::MouseInput` calls) or clicks silently drift
        // the moment `ui_scale != 1.0`.
        use crate::types::Point;
        use mkui_core::components::{Button, Mkui as CoreMkui};

        let core = CoreMkui::new().child(Button::new("Go"));
        let registry = WgpuRendererRegistry::with_defaults();
        let mut app = WgpuApp::with_app_tree(core, registry);
        app.set_surface_size(1600, 1200, 1.0);
        app.set_ui_scale(2.0);
        assert!(app.update_frame());

        let hit = *app
            .hit_tests
            .first()
            .expect("the button must produce a hit-test entry");
        let logical_center = Point::new(
            hit.rect.origin.x + hit.rect.size.width / 2.0,
            hit.rect.origin.y + hit.rect.size.height / 2.0,
        );

        // The physical cursor position a real `CursorMoved` would carry
        // for a click on the button's center, at the combined factor the
        // layout was produced against (`cursor_scale_factor()` — no
        // window here, so its OS component is the 1.0 fallback; `ui_scale`
        // is the only thing distinguishing it from a bare OS factor).
        let effective = app.cursor_scale_factor();
        let physical = (
            logical_center.x as f64 * effective,
            logical_center.y as f64 * effective,
        );

        app.pointer.update_cursor_raw(physical.0, physical.1);
        let cursor = app
            .pointer
            .cursor_logical(app.cursor_scale_factor())
            .expect("cursor must be latched after update_cursor_raw");
        app.pointer.on_press(&app.hit_tests, cursor);
        assert_eq!(
            app.pointer.armed(),
            Some(hit.node),
            "a click computed via the same combined scale factor must hit the button"
        );

        // Negative control: dividing the identical physical position by
        // the bare OS factor (1.0) instead of the combined one — what the
        // `MouseInput` handler did before #179 introduced `ui_scale` —
        // must miss. Without this, the assertion above would pass even if
        // `cursor_scale_factor` were reverted to ignore `ui_scale`
        // entirely, since it would just be round-tripping one formula
        // through itself.
        app.pointer.update_cursor_raw(physical.0, physical.1);
        let bare_os_factor_cursor = app
            .pointer
            .cursor_logical(1.0)
            .expect("cursor must be latched after update_cursor_raw");
        app.pointer.on_press(&app.hit_tests, bare_os_factor_cursor);
        assert_ne!(
            app.pointer.armed(),
            Some(hit.node),
            "a click computed via the bare OS factor (ignoring ui_scale) must miss the button"
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn first_paint_skipped_requests_retry() {
        // #93 round-N+1: a Skipped first frame (surface not ready) must
        // reschedule a redraw, otherwise the window stays blank-gray until
        // the user resizes.
        let mut app = WgpuApp::new(Scene::new(Size::new(800.0, 600.0)));
        assert!(app.first_paint_pending);
        assert!(
            app.handle_render_outcome_for_redraw(RenderOutcome::Skipped),
            "first-paint Skipped must request a retry redraw"
        );
        assert!(app.first_paint_pending, "still pending until a Drawn frame");
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn first_paint_drawn_clears_pending() {
        let mut app = WgpuApp::new(Scene::new(Size::new(800.0, 600.0)));
        assert!(
            !app.handle_render_outcome_for_redraw(RenderOutcome::Drawn),
            "a Drawn frame does not force another redraw"
        );
        assert!(!app.first_paint_pending, "first paint succeeded");
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn skipped_after_first_paint_is_a_noop() {
        // Once first paint succeeds, Skipped must idle so a quiescent UI
        // doesn't spin redraws.
        let mut app = WgpuApp::new(Scene::new(Size::new(800.0, 600.0)));
        app.handle_render_outcome_for_redraw(RenderOutcome::Drawn);
        assert!(
            !app.handle_render_outcome_for_redraw(RenderOutcome::Skipped),
            "Skipped after a successful first paint must not request a retry"
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn first_paint_retry_is_capped() {
        // A surface that never becomes ready must not spin redraws forever.
        let mut app = WgpuApp::new(Scene::new(Size::new(800.0, 600.0)));
        let mut retries = 0usize;
        while app.handle_render_outcome_for_redraw(RenderOutcome::Skipped) {
            retries += 1;
            assert!(
                retries <= FIRST_PAINT_MAX_SKIP_RETRIES as usize,
                "retry loop must be bounded by the cap"
            );
        }
        assert_eq!(retries, FIRST_PAINT_MAX_SKIP_RETRIES as usize);
        assert!(
            app.first_paint_pending,
            "cap exhausted without a Drawn frame leaves first paint pending"
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn needs_reconfigure_always_requests_redraw() {
        let mut app = WgpuApp::new(Scene::new(Size::new(800.0, 600.0)));
        app.handle_render_outcome_for_redraw(RenderOutcome::Drawn);
        assert!(
            app.handle_render_outcome_for_redraw(RenderOutcome::NeedsReconfigure),
            "a reconfigured surface must drive another frame even post-first-paint"
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn rebuild_scene_from_tree_runs_the_walker() {
        use mkui_core::components::{Mkui as CoreMkui, Text};
        use mkui_runtime::TextVariant;

        let core = CoreMkui::new().child(Text::new("hi").variant(TextVariant::Heading1));
        let registry = WgpuRendererRegistry::with_defaults();
        let mut app = WgpuApp::with_app_tree(core, registry);
        app.rebuild_scene_from_tree();
        let has_text = app
            .scene()
            .primitives
            .iter()
            .any(|p| matches!(p, crate::types::Primitive::Text(_)));
        assert!(has_text, "walker should emit at least one text primitive");
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn update_frame_rebuilds_on_a_dirty_tree_then_goes_quiet() {
        use mkui_core::components::{Mkui as CoreMkui, Text};
        use mkui_runtime::TextVariant;

        let core = CoreMkui::new().child(Text::new("hp: 100").variant(TextVariant::Body));
        let registry = WgpuRendererRegistry::with_defaults();
        let mut app = WgpuApp::with_app_tree(core, registry);

        // First call: the tree is dirty from construction, so it rebuilds.
        assert!(app.update_frame());
        // Nothing changed since: no-op, every subsequent frame.
        assert!(!app.update_frame());
        assert!(!app.update_frame());
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn update_frame_rebuilds_again_after_a_tree_mutation() {
        use mkui_core::components::{Mkui as CoreMkui, Text};
        use mkui_runtime::TextVariant;

        let core = CoreMkui::new().child(Text::new("hp: 100").variant(TextVariant::Body));
        let registry = WgpuRendererRegistry::with_defaults();
        let mut app = WgpuApp::with_app_tree(core, registry);
        app.update_frame();

        let text_id = {
            let core = app.core().unwrap().borrow();
            let root = core.tree().get(core.tree().root()).unwrap();
            root.children[0]
        };
        {
            let core = app.core().unwrap();
            let mut core = core.borrow_mut();
            assert!(core.tree_mut().set_text(text_id, "hp: 90"));
        }
        assert!(app.update_frame(), "a real content change must rebuild");
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn update_frame_reports_false_and_stays_dirty_on_a_walker_error() {
        // #177 review P2: a custom renderer erroring out of `walk_app_tree`
        // must not make `update_frame` report "rebuilt" — the old scene
        // and hit-tests are left in place, and the tree must stay dirty
        // so a future frame retries instead of going silently quiet.
        use crate::bridge::{WgpuRenderCtx, WgpuRenderOutcome, WgpuRenderable};
        use mkui_core::components::Mkui as CoreMkui;
        use mkui_core::error::MkuiError;
        use mkui_runtime::Node;

        struct FailingWidget;
        impl WgpuRenderable for FailingWidget {
            fn type_name(&self) -> &str {
                "failing_widget"
            }
            fn render(
                &self,
                _node: &Node,
                _props: &serde_json::Value,
                _ctx: &mut WgpuRenderCtx<'_>,
            ) -> Result<WgpuRenderOutcome, MkuiError> {
                Err(MkuiError::rendering("boom"))
            }
        }

        let core = CoreMkui::new();
        let mut registry = WgpuRendererRegistry::with_defaults();
        registry.register(FailingWidget);
        let mut app = WgpuApp::with_app_tree(core, registry);
        {
            let core = app.core().unwrap();
            let mut core = core.borrow_mut();
            let root = core.tree().root();
            core.tree_mut()
                .push_custom(root, "failing_widget", serde_json::Value::Null, "")
                .unwrap();
        }

        assert!(
            !app.update_frame(),
            "a walker error must not be reported as a successful rebuild"
        );
        assert!(
            app.core().unwrap().borrow().tree().is_dirty(),
            "the tree must stay dirty so a future frame retries"
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn update_frame_is_a_no_op_in_scene_only_mode() {
        let mut app = WgpuApp::new(Scene::new(Size::new(800.0, 600.0)));
        assert!(!app.update_frame());
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn set_theme_is_a_no_op_for_an_identical_theme() {
        let core = mkui_core::components::Mkui::new();
        let registry = WgpuRendererRegistry::with_defaults();
        let mut app = WgpuApp::with_app_tree(core, registry);
        app.update_frame();

        let theme = *app.theme();
        app.set_theme(theme);
        assert!(
            !app.core().unwrap().borrow().tree().is_dirty(),
            "re-setting the same theme must not dirty the tree"
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn set_theme_with_a_new_theme_dirties_the_tree() {
        let core = mkui_core::components::Mkui::new();
        let registry = WgpuRendererRegistry::with_defaults();
        let mut app = WgpuApp::with_app_tree(core, registry);
        app.update_frame();

        let mut theme = *app.theme();
        theme.tokens.spacing.radius_panel += 1.0;
        app.set_theme(theme);
        assert!(
            app.core().unwrap().borrow().tree().is_dirty(),
            "a changed theme must dirty the tree so update_frame rebuilds"
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn resize_redraw_pump_starts_idle() {
        // A freshly-constructed app has a quiescent pump — no redraw is
        // requested until a resize event arms it.
        let app = WgpuApp::new(Scene::new(Size::new(800.0, 600.0)));
        assert_eq!(app.resize_redraw_pending, 0);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn arm_resize_redraw_pump_sets_to_max() {
        let mut app = WgpuApp::new(Scene::new(Size::new(800.0, 600.0)));
        app.arm_resize_redraw_pump();
        assert_eq!(app.resize_redraw_pending, RESIZE_REDRAW_PUMP_TICKS);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn arm_resize_redraw_pump_resets_not_stacks() {
        // Codex Q2: subsequent arms reset to max, no stacking.
        let mut app = WgpuApp::new(Scene::new(Size::new(800.0, 600.0)));
        app.arm_resize_redraw_pump();
        app.arm_resize_redraw_pump();
        app.arm_resize_redraw_pump();
        assert_eq!(app.resize_redraw_pending, RESIZE_REDRAW_PUMP_TICKS);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn consume_resize_redraw_frame_decrements() {
        // The pump is armed by being asked (read-only) but drained only by
        // consuming a presented frame.
        let mut app = WgpuApp::new(Scene::new(Size::new(800.0, 600.0)));
        app.arm_resize_redraw_pump();
        assert!(app.resize_redraw_pump_armed());
        app.consume_resize_redraw_frame();
        assert_eq!(app.resize_redraw_pending, RESIZE_REDRAW_PUMP_TICKS - 1);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn consume_resize_redraw_frame_caps_at_zero() {
        // Cap prevents unbounded redraw spinning — consume N+1 frames, pump
        // must decay to 0, report disarmed, and not underflow.
        let mut app = WgpuApp::new(Scene::new(Size::new(800.0, 600.0)));
        app.arm_resize_redraw_pump();
        for _ in 0..RESIZE_REDRAW_PUMP_TICKS {
            app.consume_resize_redraw_frame();
        }
        assert_eq!(app.resize_redraw_pending, 0);
        assert!(!app.resize_redraw_pump_armed());
        app.consume_resize_redraw_frame(); // safe; no underflow
        assert_eq!(app.resize_redraw_pending, 0);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn drawn_decrements_resize_redraw_pending() {
        // StoneSketch-style (operator's 2026-06-09 deep-read): the pump
        // decays on presented frames, so a `Drawn` outcome consumes exactly
        // one frame of the budget. Note this DECREMENTS by one — it does not
        // CLEAR the pump (the earlier Codex Q3 "Drawn does not clear" still
        // holds; "decrement by 1" is the corrected mechanism).
        let mut app = WgpuApp::new(Scene::new(Size::new(800.0, 600.0)));
        app.arm_resize_redraw_pump();
        let pre = app.resize_redraw_pending;
        let _ = app.handle_render_outcome_for_redraw(crate::render::RenderOutcome::Drawn);
        assert_eq!(
            app.resize_redraw_pending,
            pre - 1,
            "Drawn must decrement the resize pump by exactly one frame"
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn skipped_does_not_consume_resize_redraw_budget() {
        // StoneSketch-style: skipped frames don't burn the pump budget.
        // Critical for fast-drag scenarios where GPU pressure may cause
        // some frames to skip — the pump must keep retrying without
        // counting skipped frames against the active-redraw window.
        let mut app = WgpuApp::new(Scene::new(Size::new(800.0, 600.0)));
        app.arm_resize_redraw_pump();
        let pre = app.resize_redraw_pending;
        let _ = app.handle_render_outcome_for_redraw(crate::render::RenderOutcome::Skipped);
        assert_eq!(
            app.resize_redraw_pending, pre,
            "Skipped must not decrement resize pump"
        );
    }

    // ---- #101: active-resize-window guard for the CursorMoved M2 bridge ----

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn active_resize_window_closed_until_first_arm() {
        // A freshly-constructed app has never resized: idle cursor moves must
        // NOT schedule redraws (idle-quiescence invariant).
        let app = WgpuApp::new(Scene::new(Size::new(800.0, 600.0)));
        assert_eq!(app.last_resize_frame, None);
        assert!(
            !app.in_active_resize_window(),
            "cursor moves before any resize must stay quiescent"
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn arm_opens_active_resize_window() {
        // M2: a cursor move within the active-resize window schedules a redraw.
        // Arming on `Resized` opens the window, so the guard reads true
        // immediately afterward.
        let mut app = WgpuApp::new(Scene::new(Size::new(800.0, 600.0)));
        app.arm_resize_redraw_pump();
        assert_eq!(app.last_resize_frame, Some(0));
        assert!(
            app.in_active_resize_window(),
            "cursor-during-active-resize-window must schedule a redraw"
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn active_resize_window_open_until_timeout() {
        // The window stays open for the full RESIZE_ACTIVE_WINDOW_FRAMES span
        // (so the cursor bridge spans the gap between successive `Resized`
        // events during a fast drag).
        let mut app = WgpuApp::new(Scene::new(Size::new(800.0, 600.0)));
        app.arm_resize_redraw_pump();
        // The last frame still inside the window.
        app.frame_counter = RESIZE_ACTIVE_WINDOW_FRAMES - 1;
        assert!(
            app.in_active_resize_window(),
            "window must remain open up to the frame before the timeout"
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn active_resize_window_expires_after_timeout() {
        // Once the timeout elapses, idle cursor moves must NOT schedule
        // redraws — independent of pump state. This is the load-bearing
        // idle-quiescence guard: the gesture ended, so the M2 bridge closes.
        let mut app = WgpuApp::new(Scene::new(Size::new(800.0, 600.0)));
        app.arm_resize_redraw_pump();
        app.frame_counter = RESIZE_ACTIVE_WINDOW_FRAMES;
        assert!(
            !app.in_active_resize_window(),
            "cursor-outside-window must NOT schedule a redraw once the timeout elapses"
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn active_resize_window_guard_independent_of_drained_pump() {
        // Codex 2026-06-09 #101 review P1#2: the guard must NOT be the circular
        // `resize_redraw_pending > 0` predicate. Drain the pump completely but
        // stay within the frame timeout — the window must remain open so the
        // cursor bridge can still fire when the pump has no budget left.
        let mut app = WgpuApp::new(Scene::new(Size::new(800.0, 600.0)));
        app.arm_resize_redraw_pump();
        for _ in 0..RESIZE_REDRAW_PUMP_TICKS {
            app.consume_resize_redraw_frame();
        }
        assert!(!app.resize_redraw_pump_armed(), "pump fully drained");
        assert!(
            app.in_active_resize_window(),
            "active-resize window must outlive a drained pump (not circular)"
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn re_arm_reopens_active_resize_window_after_partial_decay() {
        // A fresh `Resized` mid-gesture renews the window from the current
        // frame — mirroring the pump's reset-not-stack arm semantics.
        let mut app = WgpuApp::new(Scene::new(Size::new(800.0, 600.0)));
        app.arm_resize_redraw_pump();
        app.frame_counter = RESIZE_ACTIVE_WINDOW_FRAMES - 1;
        assert!(app.in_active_resize_window());
        // Another Resized arrives just before expiry: re-stamp to "now".
        app.arm_resize_redraw_pump();
        assert_eq!(app.last_resize_frame, Some(RESIZE_ACTIVE_WINDOW_FRAMES - 1));
        // Advance to what would have been expiry under the first arm.
        app.frame_counter = RESIZE_ACTIVE_WINDOW_FRAMES;
        assert!(
            app.in_active_resize_window(),
            "a fresh Resized must renew the active-resize window"
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn active_resize_window_saturates_no_underflow() {
        // `frame_counter` saturates rather than wrapping; the guard's
        // `saturating_sub` must never panic or read true for a stale arm even
        // at the counter ceiling.
        let mut app = WgpuApp::new(Scene::new(Size::new(800.0, 600.0)));
        app.frame_counter = u32::MAX;
        app.arm_resize_redraw_pump();
        assert!(
            app.in_active_resize_window(),
            "armed at the ceiling is fresh"
        );
        // about_to_wait would saturate here, not wrap; the window then closes
        // because the arm frame can never advance past the saturated counter.
        assert_eq!(app.frame_counter.saturating_add(1), u32::MAX);
    }

    // ---- #101: M2 bridge is macOS-only (Codex round-N P1) ----

    #[cfg(target_os = "macos")]
    #[test]
    fn cursor_moved_bridge_fires_inside_window_on_macos() {
        // On macOS — the sole platform with the #101 fast-shrink residual —
        // a CursorMoved inside the active-resize window DOES drive the bridge.
        let mut app = WgpuApp::new(Scene::new(Size::new(800.0, 600.0)));
        app.arm_resize_redraw_pump();
        assert!(app.in_active_resize_window(), "precondition: window open");
        assert!(
            app.cursor_moved_should_request_redraw(),
            "macOS: cursor-during-active-resize-window must drive the M2 bridge"
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn cursor_moved_bridge_quiescent_outside_window_on_macos() {
        // Even on macOS, the bridge stays quiescent once the window expires —
        // the idle-quiescence invariant holds regardless of platform.
        let mut app = WgpuApp::new(Scene::new(Size::new(800.0, 600.0)));
        app.arm_resize_redraw_pump();
        app.frame_counter = RESIZE_ACTIVE_WINDOW_FRAMES;
        assert!(
            !app.in_active_resize_window(),
            "precondition: window expired"
        );
        assert!(
            !app.cursor_moved_should_request_redraw(),
            "macOS: cursor outside the active-resize window must not redraw"
        );
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn cursor_moved_bridge_disabled_on_non_macos() {
        // Codex round-N P1: the M2 bridge is scoped to macOS. On Windows/Linux,
        // even with the active-resize window wide open, a CursorMoved must NOT
        // drive a redraw — those platforms have no fast-shrink residual and the
        // brief requires them unchanged. The shared state machine itself still
        // works (`in_active_resize_window` reads true); only the bridge gates.
        let mut app = WgpuApp::new(Scene::new(Size::new(800.0, 600.0)));
        app.arm_resize_redraw_pump();
        assert!(
            app.in_active_resize_window(),
            "shared state machine still tracks the window on non-macOS"
        );
        assert!(
            !app.cursor_moved_should_request_redraw(),
            "non-macOS: the M2 cursor bridge must be compiled out / disabled"
        );
    }
}
