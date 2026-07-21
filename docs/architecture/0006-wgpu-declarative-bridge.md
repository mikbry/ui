# ADR 0006 — `mkui-wgpu` declarative bridge over `mkui-runtime::AppTree`

## Status

Accepted (Sprint 5, 2026-05-27).

## Context

After Sprint 4 ([ADR 0005](0005-mkui-runtime-portable-substrate.md))
shipped `mkui-runtime` — the arena-backed `AppTree` substrate every
binding builds into — five of the six backends consumed it directly:
`mkui-web` walked it into the DOM, `mkui-console` flattened it for the
terminal, `mkui-c` / `mkui-py` built into it, `mkui-native` re-exported it.

`mkui-wgpu` was the odd one out. It still exposed only the pre-substrate
HUD primitive API — `Mkui::with_scene(scene).run()` — inherited from the
2D HUD pipeline port ([ADR 0004](0004-mkui-wgpu-2d-hud-pipeline-port.md)).
A wgpu user could not write the same declarative
`Mkui::new()?.child(View::new().child(...))` they wrote against the web
and console backends. `examples/showcase-common::create_showcase_ui` —
the cross-binding canonical demo — only ran on web + console.

The closed-not-merged PR #50 attempted this bridge pre-substrate; the
design surfaced architectural gaps that Sprint 4 closed. This sprint
finishes the job against the substrate that now exists.

## Decision

Implement the wgpu declarative bridge as a thin layer on top of the
existing tessellation pipeline.

### Cross-binding identity: one primary backend

The `mkui` bridge crate's identity is "depend on `mkui`, pick a backend with a
Cargo feature, get one `mkui::Mkui` type." That identity holds only if the
backend selection is unambiguous. The `web`, `console`, and `wgpu` features
each resolve `Mkui` to a *different* concrete type and rendering path, so they
are **mutually exclusive primary backends**.

Earlier the bridge resolved a multi-feature build by hidden `cfg` precedence
(`console` shadowed `web`, which shadowed `wgpu`). That is unsafe: Cargo
feature unification across a dependency graph can enable a second backend
feature a consumer never asked for, and the build would *silently* select a
different backend than intended — a cross-binding identity violation with no
diagnostic. As of v0.10.0 (#102) the invariant is enforced at compile time:

```rust
#[cfg(any(
    all(feature = "web", feature = "console"),
    all(feature = "web", feature = "wgpu"),
    all(feature = "console", feature = "wgpu"),
))]
compile_error!(
    "mkui: enable exactly one primary backend feature: `web`, `console`, or `wgpu`"
);
```

**The rule:** enable *exactly one* primary backend feature. Every conflicting
pair (and the all-three combo) is a hard compile error; there is **no silent
precedence**. Zero backends remains valid — `Mkui::new()` returns a clear
`MkuiError` naming which feature to enable, so a library consumer who forgot
to pick one gets a runtime explanation rather than a link error. Each valid
single-backend build therefore has exactly one `inner` field and one
implementation path through the bridge.

This is a pre-1.0 breaking change for any consumer that was (knowingly or not)
relying on the old precedence. Because the backend is selected only at compile
time, this guard adds no runtime backend-selection API, dependency, or MSRV
change. CI enforces it package-aware (`-p mkui --no-default-features`, never
workspace `--all-features`, which would mask the conflict via unification):
the four valid builds must compile and the four conflicting combos must fail
with the message above.

### Component placement

- **`WgpuRenderable` trait + `WgpuRendererRegistry`** live in
  `mkui-wgpu`, **not** `mkui-runtime` or `mkui-core`. This mirrors
  [`mkui-web::WebRendererRegistry`](../../crates/mkui-web/src/render.rs)
  and matches the Codex round-7 Q1 ratification (renderer traits are
  backend-local; the substrate stays renderer-agnostic).
- **Walker** (`crates/mkui-wgpu/src/walker.rs`) — consumes
  `mkui_runtime::AppTree` and emits scene primitives via the existing
  tessellation pipeline. No new GPU pipeline state is introduced; the
  pipeline boundary documented in ADR 0004 (`Scene` → HUD pass) holds.
- **Input router** (`crates/mkui-wgpu/src/pointer.rs`) — cursor tracked
  from `WindowEvent::CursorMoved`, hit-test in reverse paint order
  against the per-frame `Vec<HitTestEntry>` collected during the walk,
  click semantics on release. Logical/physical DPI conversion uses the
  window's `scale_factor`.
- **Built-in registrations** at `WgpuRendererRegistry::with_defaults()`
  ship `BadgeRenderer` + `DotRenderer` so the scene-primitive atoms
  ported in ADR 0004 stay accessible through the AppTree path. The
  built-in node kinds (`View` / `Text` / `Button`) render through fixed
  paths in the walker.

### Eager rebuild on dirty signal

Frames are rebuilt eagerly when the runtime's dirty flag is set. There
is **no** incremental tree diffing in v0.6.0 — diffing without a
performance baseline is premature optimisation (Codex round 7). When an
action fires, it sets `RuntimeCtx::mark_dirty`; the wgpu input handler
observes the flag and calls `window.request_redraw()` from inside the
event loop — **never** from inside the action closure (Sprint 4
anti-pattern guard).

### Closure capture

Hit entries carry cloned `Option<ActionId>` handles, not references
into the tree. Actions fire through `tree.actions().fire(id)` only
after the per-frame hit-test borrow has ended — collect first, invoke
after. This is the Sprint 4 retro's load-bearing rule applied forward;
violating it would re-introduce the "closure holding `RefCell` borrow
across rebuild" failure mode the substrate explicitly closed.

### `Mkui::with_scene` as the retained low-level escape hatch

`Mkui::with_scene(scene).run()` is **retained as the low-level escape
hatch**, not deprecated. The declarative `Mkui::new()?.child(...).run()`
API is the public identity end users build their apps against; the
wgpu-specific `Scene` constructor coexists for renderer tests, custom
HUDs, headless tessellation demos, and future direct-GPU experiments
(Slug glyph rendering, mkui-vector2d primitives — Sprint 7+). Both
surfaces are documented public API.

This matches the Codex round-10 Q5 ratification and Sprint 5
acceptance criterion #14. The alternative — deprecating `with_scene`
in v0.6.0 — would force consumers building renderer tests or custom
HUDs onto a workaround path before the cross-binding declarative API
covers their use case. The declarative API is the *recommended* surface
for app code; it is not the *only* surface.

The wgpu backend's `Scene` API is intentionally an
implementation-of-the-HUD-pipeline surface, not a cross-binding
contract. That is the source of the asymmetry vs. web/console (which
have no equivalent low-level surface): the HUD pipeline (ADR 0004) is
the load-bearing wgpu-specific layer, and giving it a public
constructor is the right way to expose it without re-exporting the
tessellator's internals.

### Resize behaviour contract

The two construction paths above carry **different** resize contracts, and
the `WindowEvent::Resized` handler must honour both (this was left implicit
in Sprint 5 and broke the raw-scene path — #93):

- **Declarative / AppTree path** (`Mkui::new` / `from_core`): the scene is a
  per-frame projection of the runtime tree. On resize the handler discards
  the scene, marks the tree dirty, and **eagerly rebuilds** from the tree
  against the new viewport — consistent with the eager-rebuild-on-dirty model
  above. Primitives that depend on the viewport re-flow correctly because the
  walker re-runs.
- **Raw-scene escape hatch** (`Mkui::with_scene`): the user owns the scene.
  Its contract is "I handed you primitives; render them across resizes." The
  handler must **preserve** those primitives and only update `Scene::viewport`
  in place — it must not replace the scene with a fresh empty one (there is no
  tree to rebuild from, so a replacement is a permanent wipe). An in-place
  `Scene::viewport` update is sufficient — and, since #97, also *necessary*:
  `Renderer::render` projects against `Scene::viewport` (see §"Viewport units
  contract"), so the in-place update keeps the projection denominator current.

Future work that touches the resize handler must keep both branches intact;
the `WgpuApp::resize_scene_viewport` helper is the single seam that encodes
this contract, and the #93 regression tests assert each branch.

### Viewport units contract

`Scene::viewport` and all primitive coordinates (`Quad::rect`, `Text::rect`,
`Icon::rect`, custom-node `x`/`y` props, walker layout positions) are in
**logical pixels**. This matches:

- `mkui-web`'s CSS-pixel convention (DOM positioning)
- `mkui-console`'s character-grid logical convention
- The cross-binding identity contract: the same primitive at logical
  `(200, 150)` lands at the same visible-screen position on web, console,
  and wgpu (modulo each backend's pixel-density characteristics).

The wgpu backend reconciles this with the GPU's physical-pixel surface as
follows:

- `Renderer::config.{width, height}` configure the wgpu surface in
  **physical pixels** (required by wgpu's `surface.configure` contract).
- `gui_vertices` projects logical-pixel coordinates to NDC using
  `scene.viewport.{width, height}` (logical) as the denominator. wgpu /
  the rasterizer then maps NDC to device pixels.
- On `WindowEvent::Resized`, the resize handler reconfigures the surface
  in physical pixels AND updates `Scene::viewport` to the new logical
  viewport (`physical / scale_factor`).
- On `WindowEvent::ScaleFactorChanged`, the same conversion runs with the
  new `scale_factor` so a window dragged between displays of different
  DPIs stays correctly proportioned.

The `physical / scale_factor` conversion lives in one place — the
`logical_viewport_from_physical_size` free function in `app.rs` — shared by
both the `Resized` and `ScaleFactorChanged` arms. Fractional scale factors
(e.g. 1.5×) are preserved as f32 floats; we do not round to integer logical
pixels.

Before #97, `Renderer::render` mistakenly projected against
`self.config.{width, height}` (physical), so on any `scale_factor > 1`
display, logical-pixel primitives mis-projected into the upper-left quadrant.
The fix was a single load-bearing change at the projection call site — the
renderer already received the logical viewport via `scene.viewport`; it just
stopped reading the wrong field.

### First-paint render scheduling

`resumed()` creates the window and schedules exactly one redraw. wgpu's
`Renderer::render` returns `RenderOutcome::Skipped` when the surface is not
yet ready (Timeout / Occluded / Validation), and a UI-framework event loop
has no other redraw trigger while idle — so a `Skipped` first frame left both
examples blank-gray until the user resized or interacted (#93 round-N+1).

The contract: **the first successfully-`Drawn` frame is guaranteed to be
scheduled.** While `first_paint_pending`, a `Skipped` outcome reschedules
another redraw (capped by `FIRST_PAINT_MAX_SKIP_RETRIES` so a permanently
occluded surface can't spin). The first `Drawn` clears the flag, after which
`Skipped` returns to a no-op so a quiescent UI idles rather than busy-redraws.
`NeedsReconfigure` always drives another frame. This logic lives in the
`WgpuApp::handle_render_outcome_for_redraw` seam, asserted by unit tests; it
deliberately does **not** adopt StoneSketch's redraw-after-every-input-event
pattern, which would defeat idle-frame quiescence for a UI framework.

### Resize-active redraw pump

macOS fires `WindowEvent::Resized` continuously during live-resize gestures.
Each event triggers `Renderer::resize` (which reconfigures the surface) plus
`window.request_redraw()`. The OS may present a frame between the
reconfiguration and the next rendered output — on shrinking gestures
(bottom→top, right→left) this produces a visible "scale-snap" jerk as
the Metal swapchain transitions before the next frame at the new size
is ready.

A wgpu+winit upstream reference does not have this jerk because it drives
continuous redraws via `about_to_wait` while a progressive accumulator
is active. mkui-wgpu cannot follow that pattern directly — it's a UI
framework where idle windows must idle (not a 3D editor with continuous
accumulation).

The mitigation is a **narrow arm-and-decay pump**: armed on `Resized` +
`ScaleFactorChanged` to a fixed cap (`RESIZE_REDRAW_PUMP_TICKS = 60`).
The resize handlers only arm the pump; they do not directly call
`window.request_redraw()`. While armed, `about_to_wait` calls
`window.request_redraw()` after the current event batch drains — a pure read
of the pump state, it does not drain the budget.

> **Layer note (#101).** The pump and the `CursorMoved` M2 bridge below both
> operate at the **event-scheduling layer** — they change *when* redraws are
> requested. Operator visual-verify proved they cannot eliminate the macOS
> fast-shrink residual, because its root cause is at the **presentation layer**
> (see "Root cause" immediately below). The event-layer work is retained as
> correct, orthogonal hygiene (it keeps redraw cadence sane during a gesture
> and is correctly macOS-scoped), but the load-bearing fix is
> `presentsWithTransaction`.

#### Root cause: CAMetalLayer `presentsWithTransaction` (#101, presentation layer)

The macOS fast-resize jerk is a **presentation-layer race**, not an
event-scheduling one. AppKit commits a window's new bounds inside a
`CATransaction`; by default (`CAMetalLayer.presentsWithTransaction = false`)
the layer presents its drawable on an independent, asynchronous timeline. During
a live-resize the window rect jumps to the new size before the matching drawable
is scheduled, so Core Animation stretches the previous (stale-size) drawable into
the new rect for one or more frames — the visible vibration. Neither winit nor
wgpu set `presentsWithTransaction` by default (winit#3644).

The fix (`render::enable_presents_with_transaction`, macOS-only, called once
after `surface.configure`): reach the surface's `CAMetalLayer` via
`wgpu::Surface::as_hal::<hal::api::Metal>()` and flip
`setPresentsWithTransaction(true)`. wgpu-hal's Metal `Queue::present` already
branches on this flag — when set, it commits the command buffer,
`waitUntilScheduled()`, then calls `drawable.present()` *inside the current
`CATransaction`* (wgpu-hal 29 `src/metal/mod.rs`). The drawable resize and the
window-rect resize then commit atomically, with no stretch frame. The flag is a
layer property (persists across `configure`) re-read every `acquire_texture`, so
one call at surface creation suffices.

This is the sole `unsafe` seam in `mkui-wgpu`: the crate is `#![deny(unsafe_code)]`
(downgraded from `forbid` for exactly this FFI call), and the function carries a
scoped `#[allow(unsafe_code)]` + `SAFETY:` note. No public API change; every
non-macOS backend keeps the standard wgpu present path (the call is compiled out
by `#[cfg(target_os = "macos")]`). It cannot be unit-tested without a live Metal
surface (displayless CI has none; GPU tests are Vulkan/Lavapipe-only), so its
binding verification is operator visual-verify on Retina.

References: Tristan Hume, "Glitchless Metal Window Resizing"; Raph Levien, "The
smooth resize test"; wgpu#1168 ("Resizing on macOS is cursed again"); winit#3644.

#### CursorMoved M2 bridge during the active-resize window (#101, event layer — orthogonal hygiene)

The `Resized`-armed pump (cap = 60) reached its operator-verified ceiling at
v0.9.3: a small residual vibration persisted on *fast shrink-direction* drags
(right→left, bottom→top). Three hypotheses were ruled out during PR #100's
iteration — pump-cap exhaustion (cap=120 was *worse*, not better), scene
complexity by primitive count, and absolute-vs-relative primitive coords — so
the residual is **not** a budget problem and a further cap tune cannot close it.

#101 adds a second, orthogonal redraw trigger: **M2 — immediate redraw
scheduling on `CursorMoved` during a recent-resize-active window.** When the
cursor moves inside the active-resize window, the `CursorMoved` handler calls
`window.request_redraw()` *directly*, independent of the pump's counter. This
gives the redraw cadence a cursor-driven trigger to complement the pump's
`Resized`-arm trigger, smoothing the cadence toward cursor motion rather than
the sparser `Resized` event arrival.

Two design constraints from the Codex 2026-06-09 #101 review shaped this:

- **M2, not M1.** Re-arming the pump on cursor events (M1) would only help if
  cap-exhaustion were the bottleneck — it is ruled out — so M2 schedules a
  redraw *without* touching `resize_redraw_pending`. (The pre-#101 code re-armed
  the pump on `CursorMoved`; that M1 path is replaced by M2.)
- **The guard must be pump-state-independent.** Encoding "are we resizing?" as
  `resize_redraw_pending > 0` is circular: a drained pump reads as "not
  resizing" exactly when the residual vibration needs cursor-driven redraws.
  The guard is instead an explicit frame-count timeout (G2): `about_to_wait`
  advances a monotonic `frame_counter`; `arm_resize_redraw_pump` stamps
  `last_resize_frame = frame_counter`; and `in_active_resize_window()` is true
  while `frame_counter - last_resize_frame < RESIZE_ACTIVE_WINDOW_FRAMES` (60,
  ~1s at 60Hz). The window opens on `Resized`/`ScaleFactorChanged` and closes
  on the timeout — keeping the cursor bridge live across a fast gesture while
  preserving idle quiescence once the drag stops. `frame_counter` saturates
  rather than wraps.

Both pieces of state (`resize_redraw_pending`, `last_resize_frame`) are armed
together but decay independently: the pump drains on presented `Drawn` frames;
the active-resize window expires on the frame-count timeout. These invariants
are asserted by unit tests (`active_resize_window_expires_after_timeout`,
`active_resize_window_guard_independent_of_drained_pump`).

The M2 bridge is **macOS-only**. The fast-shrink residual is a macOS Metal
swapchain artifact; Windows and Linux do not exhibit it, and the brief scopes
the fix to macOS. So the *bridge action* — `CursorMoved → request_redraw()` —
is compile-time gated with `#[cfg(target_os = "macos")]` in
`cursor_moved_should_request_redraw`, folding to a constant `false` on other
native targets (no runtime branch, no behavior change on Win/Linux). The
active-resize-window **state machine** (`frame_counter`, `last_resize_frame`,
`in_active_resize_window`) is *not* gated — it compiles, runs, and is
unit-tested identically on every native target; only the platform-specific
redraw it gates is conditional. `cursor_moved_bridge_disabled_on_non_macos`
asserts the off-macOS disabled path; `cursor_moved_bridge_fires_inside_window_on_macos`
asserts macOS keeps the behavior.

The budget is consumed one frame at a time on each successful
`RenderOutcome::Drawn` (in `handle_render_outcome_for_redraw`). `Skipped`
and `NeedsReconfigure` do **not** consume budget. `RenderOutcome::Drawn`
decrements the pump by one but does NOT clear it: mkui draws once per resize
event, so clearing on the first `Drawn` would make the pump a no-op (it would
reset to idle before bridging the Metal swapchain transition).

`RedrawRequested` also performs a final size sync before rendering: it reads
the current `window.inner_size()`, and if the renderer config is stale, it
reconfigures the surface and updates the logical scene viewport before
building vertices. This covers the fast-drag race where AppKit/CoreAnimation
has already advanced the native layer size but the matching winit `Resized`
event has not yet drained through the application handler.

This matches a proven upstream wgpu+winit reference's accumulation-frame-cap
mechanism, where the active-redraw window measures **successful presented
frames, not event-loop ticks**. Under GPU pressure during fast resize,
some frames skip; if skipped frames burned the budget, the bridge would
exhaust before the gesture ends and the visible jerk would persist.

The cap is **60**, not a handful of frames. It was tuned during Sprint 6.5
(#99 → #100) as operator visual verify narrowed the residual symptom: a
4-frame window kept up with slow drags but exhausted faster than `Resized`
events re-armed it during *fast* gestures (full jerk); 60 matched the
upstream reference's proven cap and dropped the symptom to a small residual
vibration on fast drags. The cap was briefly raised to 120 during the
iteration, but operator visual verify showed 120 was **worse** than 60 —
likely GPU/swapchain saturation from over-queued redraws presenting against
stale surface configs — so the cap reverted to 60. Each `Resized` event
re-arms to the cap, so the pump stays saturated for the whole live-resize
gesture and only begins decaying once the drag stops — then returns to idle
quiescence within ~60 presented frames (~1s at 60Hz). The value fits `u8`.
The 120 result refutes "pump exhaustion" as the residual root cause; the
remaining fast-drag vibration is addressed by the **#101** shape change (the
`CursorMoved` M2 bridge during the active-resize window, documented above) —
not a further cap tune.

The renderer also treats `CurrentSurfaceTexture::Suboptimal` as a resize-
recovery signal: it renders and presents the frame, then immediately
reconfigures the surface against the current config. That mirrors the
upstream reference and prevents a live-resize sequence from continuing to
present against a surface wgpu has already marked as no longer ideal.

The pump is **independent** of the first-paint state machine
(`first_paint_pending`). Both live on `WgpuApp` and both observe the same
`Drawn` outcome, but they track orthogonal state: `Drawn` clears
`first_paint_pending` (a one-shot flag) AND decrements `resize_redraw_pending`
(a frame counter) — neither reads the other. A `Skipped` frame may retry
first-paint while leaving the resize pump untouched. These invariants are
asserted by unit tests (`drawn_decrements_resize_redraw_pending`,
`skipped_does_not_consume_resize_redraw_budget`).

### Color space + blending (linear-in / sRGB-at-boundary) (#135)

**Contract: render in linear light; encode to sRGB once, at the surface
boundary.** Every lane (UI triangles, bitmap text, Slug curves) composites into
a **linear-space intermediate framebuffer** (`INTERMEDIATE_FORMAT =
Rgba8Unorm`, a plain linear UNORM with no hardware sRGB encode on write). A
final full-screen **present pass** (`render/present.wgsl`) samples that linear
result and writes it to the swapchain, applying the linear→sRGB OETF only when
the surface is UNORM; when the surface is sRGB (the preferred, common case) the
present fragment returns the linear value and the swapchain view performs the
single encode. There is exactly one encode, ever.

**Why.** Alpha blending is only physically correct in linear light. The
pre-Sprint-8 renderer composited straight into an sRGB swapchain; the UI lane
pre-linearized its vertex colors on the CPU (so it leaned on the driver's
sRGB-target blend), but the **Slug lane did not linearize its glyph color at
all** — so anti-aliased outline text composited in a different space than the UI
around it, and partial-coverage edges darkened. Blending in the wrong space is
mathematically incorrect, not merely an aesthetic preference (do **not**
"fix" it by darkening colors). Moving all compositing into a linear
intermediate makes the space uniform and correct for every lane by
construction.

**Color literals are sRGB-perceptual; alpha is linear.** The color-literal
audit (#135) found every `Color` literal in the tree — the ~50-token
`theme.rs` palette, component defaults, example fills — is a designer-picked
**sRGB perceptual** value, and every alpha is a linear coverage weight. So the
model is: `Color` stores sRGB; the render boundary converts to linear exactly
once via [`Color::to_linear_rgba`] (`srgb_to_linear` on RGB, alpha passthrough).
No literal stores linear channels, so **no literal needed rewriting**.
`Color::from_srgb` / `from_srgb_a` are intent-signalling constructors (identical
storage to `rgb`/`rgba`) for call sites that want the space explicit. The
conversion happens in three symmetric places — `gui_vertices` (triangle
colors), `scene_slug_glyphs` (Slug fill color), and `clear_color` (backdrop) —
never per-literal at construction.

**A/B verification.** The GPU acceptance test `render/linear_blend_gpu.rs`
(Lavapipe lane) drives the real UI pipeline into the linear target and reads
back raw bytes: an authored sRGB `0.5` gray stores as byte **~55** (linear
0.214), *not* the byte **128** the old sRGB-space compositing produced — a
direct numeric A/B of the two color pipelines. A second test confirms white at
50% alpha over black is the linear midpoint (byte ~128). The perceptual
before/after screenshot of `examples/text --features slug` (visible edge
darkening on `M`/`g` diagonals gone) is the deferred operator visual-verify
(the windowed present pass has no offscreen surface to assert against; its
shader is naga-validated in the default test job).

**Interaction with MSAA (#134).** MSAA now belongs *inside* the linear
intermediate: the multisampled attachment resolves into the linear
intermediate (a correct linear→linear average) and the present pass performs
the sole sRGB encode afterward — so the resolve step can never double-encode.
The machinery is shaped for that today (the resolve target is already the
intermediate view) but MSAA stays pinned off here; re-enabling it is #134's
scope, not this change's.

**Sprint 6 double-encode, closed by construction.** The v0.9.1 CHANGELOG noted
an MSAA-resolve-into-sRGB step double-applying encoding
(`srgb_encode(srgb_to_linear(c))`) on macOS Metal. With a linear intermediate
there is no sRGB texture in the compositing path to decode/re-encode, so the
double-encode is structurally impossible — not merely patched.

### MSAA disabled pending correct sRGB orchestration

The UI pass currently runs at `sample_count = 1` (`MSAA_SAMPLE_COUNT_PREF = 1`
in `render/mod.rs`). This is a **deliberate temporary policy**, not the target
state:

- The Sprint 5 4× MSAA path has **no StoneSketch upstream parent** — the
  reference HUD pipeline runs at `MultisampleState::default()` (`sample_count
  = 1`). MSAA was a mkui-wgpu-local addition.
- It is the suspected source of two #93 symptoms on macOS Metal: the gray
  backdrop darkening on every resize, and `atoms-on-wgpu` rendering empty
  despite emitting thousands of valid triangles at the CPU stage — both
  consistent with the MSAA-resolve-into-sRGB step double-applying sRGB
  encoding (`srgb_encode(srgb_to_linear(c))`).
- `sample_count = 1` writes the swapchain view directly with no resolve step,
  which is the StoneSketch-proven, visually-correct path.

The MSAA machinery (`pick_sample_count`, `create_msaa_color_view`, the
`msaa_color_view` attachment) is retained but dormant so the policy can be
reversed cleanly. Since #135 (see §"Color space + blending") the render pass
composites into a **linear intermediate**, which is exactly the correct home
for MSAA: the multisampled attachment resolves into that linear target (a
linear→linear average) and the separate present pass performs the sole sRGB
encode — no resolve-into-sRGB double-encode is possible. The `msaa_color_view`
resolve target is already wired to the intermediate view; re-enabling
anti-aliasing is now just flipping `MSAA_SAMPLE_COUNT_PREF`, tracked in **#134**
(closer of **#95**). Until that lands, anti-aliasing is intentionally off.

### Layout v1

The walker implements a deliberately small layout: top-down vertical
flow with class-driven padding + gap + text/button sizing. This is the
minimum that renders `examples/showcase-common::create_showcase_ui`
recognisably end-to-end. Full flex / grid layout is deferred (Sprint
7+); the eager-rebuild model lets a richer layout drop in without
re-architecting the walker.

## Consequences

- `examples/showcase-common::create_showcase_ui` now renders on the
  wgpu backend through the same declarative API the web and console
  backends use. `examples/showcase-common/src/lib.rs` is byte-unchanged
  (Codex round-7 Q6 ratification).
- `examples/atoms-on-wgpu` is re-introduced — the 12-badge grid + dot
  showcase + title text from the v0.5.x closed-not-merged #48 demo,
  now built through `tree.push_custom("badge", …)` /
  `tree.push_custom("dot", …)`. The renderers ship in
  `WgpuRendererRegistry::with_defaults()` so users get them without
  registration.
- `examples/native-showcase` is added — the cross-binding canonical
  showcase running on wgpu via `mkui::run!(create_showcase_ui, wgpu)`.
- `mkui-wgpu::Mkui::with_scene` is **retained** as the documented
  low-level escape hatch. Recommended app code uses the declarative
  `Mkui::new()?.child(...).run()` API; renderer tests, custom HUDs,
  and direct-GPU work continue to use `with_scene`. Both surfaces
  coexist as documented public API.
- The HUD tessellation pipeline (ADR 0004) is preserved verbatim. The
  bridge layers **above** it — walker → `Scene` → tessellator → HUD
  pass — so any future tessellation work (Slug-style outline text per
  ADR 0002) drops in without disturbing the bridge.
- `mkui-wgpu/Cargo.toml` gains a `mkui-runtime` dependency (Sprint 4
  deliberately deferred this edge — the wgpu backend was the only
  binding that hadn't taken the substrate).

## Alternatives considered

- **Put `WgpuRenderable` in `mkui-runtime`.** Rejected. The runtime is
  renderer-agnostic by design (Codex round-7 §8); pulling in a wgpu
  trait would force `mkui-runtime` to either depend on `wgpu` (a
  150-crate transitive tree, deeply backend-specific) or to expose
  trait-object boilerplate that the wgpu side then re-wraps. Backend-
  local placement matches the web pattern that already works.
- **Incremental tree diffing.** Rejected for v0.6.0. The substrate is
  too new — there are no criterion benches yet, no live-node counter,
  no production scenes large enough to know whether diffing pays for
  its complexity. Eager rebuild on dirty signal is the documented
  Codex round-7 baseline; diffing becomes Sprint 8+ work after the
  audit Phase 4 performance bench scaffolding lands.
- **Match the web backend's `thread_local!` active-tree pointer.**
  Rejected. The web backend's `thread_local!` exists because
  `wasm_bindgen` closures must own their captures (`'static`) and
  cannot borrow `&AppTree` into each onclick. The wgpu input handler
  has direct access to the app's `Rc<RefCell<AppTree>>` through the
  event loop, so the global pointer would be pure ceremony — and a
  re-entry hazard. Codex round 10 Q3 anti-pattern-list flagged this
  ahead of time.
- **Deprecate `Mkui::with_scene` in v0.6.0.** Rejected per Codex
  round-10 Q5. Both shapes ARE equally supported, just for different
  use cases: declarative for app code, `with_scene` for
  direct-to-renderer paths (renderer tests, custom HUDs, headless
  tessellation, future Slug / mkui-vector2d integration). Deprecating
  the low-level surface before its successor exists would force
  consumers onto a workaround path for a use case the declarative API
  is not designed to cover. The two surfaces target different layers
  of the stack and coexist by design.

## Out of scope (reserved for future sprints)

- **Shared `mkui-layout` crate / module.** This sprint ships a
  minimal **wgpu-local** layout pass inside the walker (Codex
  round-10 Q3 Option A): top-down vertical flow with class-driven
  padding + gap + text/button sizing, just enough to render
  `examples/showcase-common::create_showcase_ui` recognisably on
  wgpu. A future sprint may extract layout to a `mkui-layout` shared
  module/crate consumed by `mkui-runtime` + every backend (Codex
  round-10 Q3 Option D) once web/console/wgpu/native need true
  layout parity. The wgpu-local pass is intentionally scoped to the
  current showcase tokens; the shared layer is the right place for
  general flex / grid semantics, breakpoint resolution, and
  cross-binding layout snapshots.
- **Visual regression / screenshot diff infrastructure.** Sprint 7+.
- **Hover state, responsive breakpoints, CSS transitions.** Sprint 7+
  — depend on either the wgpu-local layout v2 or the shared
  `mkui-layout` extraction, whichever lands first.
- **Incremental tree diffing.** Sprint 8+, after criterion benches
  exist (see "Alternatives considered" above for rationale).
