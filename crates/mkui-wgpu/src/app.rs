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
use crate::walker::{walk_app_tree, HitTestEntry, WalkOptions};
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
    /// rebuild via [`walk_app_tree`]'s [`WalkOutput`]; the input router
    /// reads this slice (no `&mut` is ever handed to a renderer or
    /// action closure).
    #[cfg(not(target_arch = "wasm32"))]
    hit_tests: Vec<HitTestEntry>,
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
}

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
    /// from the tree per frame (eager rebuild — Sprint 4 substrate
    /// contract, ADR 0006).
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

    pub fn set_text_system(&mut self, text_system: Arc<dyn TextSystem>) {
        self.text_system = text_system;
    }

    pub fn theme(&self) -> &WgpuTheme {
        &self.theme
    }

    pub fn set_theme(&mut self, theme: WgpuTheme) {
        self.theme = theme;
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
        let Some(core) = self.core.as_ref() else {
            return;
        };
        let Some(registry) = self.registry.as_ref() else {
            return;
        };
        let options = WalkOptions {
            viewport: self.scene.viewport,
            theme: self.theme,
        };
        let output = {
            let core_ref = core.borrow();
            // v0.6.0 never returns Err from the walker; surface a render
            // error if a future extension renderer changes that.
            match walk_app_tree(core_ref.tree(), registry, &options) {
                Ok(out) => out,
                Err(err) => {
                    eprintln!("mkui-wgpu: walk_app_tree error: {err}");
                    return;
                }
            }
        };
        // Clear dirty after the walk completes so a future action firing
        // mid-walk does not get its dirty bit clobbered.
        core.borrow_mut().tree_mut().clear_dirty();
        self.scene = output.scene;
        self.hit_tests = output.hit_tests;
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
            logical_viewport_from_physical_size(size, state.window.scale_factor())
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
                    self.rebuild_scene_from_tree();
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
                let new_viewport = {
                    let Some(state) = self.state.as_mut() else {
                        return;
                    };
                    state.renderer.resize(size.width, size.height);
                    logical_viewport_from_physical_size(size, state.window.scale_factor())
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
                let new_viewport = {
                    let Some(state) = self.state.as_mut() else {
                        return;
                    };
                    let size = state.window.inner_size();
                    state.renderer.resize(size.width, size.height);
                    logical_viewport_from_physical_size(size, scale_factor)
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
                // The `in_active_resize_window` timeout keeps ordinary idle
                // pointer motion quiescent once the gesture stops.
                if self.in_active_resize_window() {
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
                let scale = self
                    .state
                    .as_ref()
                    .map(|s| s.window.scale_factor())
                    .unwrap_or(1.0);
                let Some(cursor) = self.pointer.cursor_logical(scale) else {
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
    /// active-resize-window guard ([`WgpuApp::in_active_resize_window`]) reads
    /// to expire the `CursorMoved` M2 bridge a fixed number of frames after the
    /// last `Resized` — keeping that bridge live across a fast gesture without
    /// redrawing on idle pointer motion afterward.
    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        self.frame_counter = self.frame_counter.saturating_add(1);
        if self.resize_redraw_pump_armed() {
            if let Some(state) = self.state.as_ref() {
                state.window.request_redraw();
            }
        }
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
}
