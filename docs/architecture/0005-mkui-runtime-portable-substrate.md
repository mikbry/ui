# ADR 0005 — `mkui-runtime` as the portable AppTree substrate

## Status

Accepted (Sprint 4, 2026-05-25).

## Context

By Sprint 3 close, the mkui workspace had four backend implementations
(`mkui-web`, `mkui-console`, `mkui-wgpu`, partial `mkui-native`) plus two
language bindings (`mkui-c`, `mkui-py`). Each had grown its own subset of
the component model:

- `mkui-c` and `mkui-py` exposed only flat `add_view` / `add_text` /
  `add_button` — no nesting, no callbacks across the FFI boundary, no
  theme/state API.
- `mkui-web` had a real `WebRendererRegistry` with custom-component
  extension; the other backends did not.
- `mkui-core` stored `Box<dyn Component>` + `Rc<dyn Fn()>` callbacks —
  ergonomic in Rust, *not portable* across C/Python (closures don't cross
  FFI).
- No shared class-string parser existed — web silently accepted whatever
  the DOM did; wgpu/console/C/Py would diverge once they cared about
  layout.

Codex round-6 follow-up + round-7 review identified this as the root
cause: the architecture was missing a *contract-implementation layer*.
`mkui-core` is the pure-contract crate (per ADR 0001); the bindings needed
a shared substrate that owns the actual storage every renderer reads.

## Decision

Add `mkui-runtime`. It owns the portable application tree:

- `AppTree` — arena-backed scene graph indexed by `NodeId`.
- `NodeId` / `ActionId` — `(index, generation)` handles that survive FFI
  round-trips and guard against use-after-free.
- `NodeKind` — `Root` / `View` / `Text` / `Button` / `Custom`. The
  `Custom { type_name, props }` slot is the extension point — downstream
  components (Sprint 6+ shadcn parity) and the Sprint 4 `TestWidget`
  extension proof flow through it.
- `ActionRegistry` — single-threaded callback table. Actions register an
  `ActionId`; the binding owns the actual callable. Renderers fire by id
  through the registry; closures never cross the FFI boundary.

  > **Update (#70):** the Codex round-7 §"Concrete Shape" design listed a
  > generation-counter + `free` reuse pool on `ActionRegistry` as a
  > use-after-free guard for a future node-removal API. That removal API
  > never shipped, so the `free` pool was dead infrastructure and has been
  > removed. The `ActionId.generation` field is retained as an always-`0`
  > forward-compat reservation (the public id shape is unchanged); the guard
  > will be re-added — `free` pool + `ActionRegistry::remove(id)` — if/when a
  > node-removal API lands.
- `StyleClass` + `ResolvedStyle` — utility-class strings and the typed
  projection. Parser supports the 39 Tier-1 tokens used by the showcase,
  documents 3 Tier-2 no-op patterns, and rejects Tier 3 as a parse error
  with a helpful message.
- `snapshot` (feature `snapshot`) — canonical JSON of an `AppTree`.
  Stable across construction frontends; parity tests assert byte-identity.

### Dependency direction

`mkui-runtime` is foundational. The new graph:

```
bindings (mkui-c, mkui-py)
    ↓
bridge (mkui)
    ↓
backends (mkui-web, mkui-console, mkui-wgpu, mkui-native)
    ↓
mkui-core  ← keeps the Rust ergonomic builder + headless logic + theme
    ↓
mkui-runtime  ← AppTree, ids, action registry, class parser, snapshots
```

`mkui-core` re-exports `ButtonVariant` / `TextVariant` / `StyleClass` from
`mkui-runtime` to preserve the historical `mkui_core::headless::*` /
`mkui_core::style::*` paths. The Rust ergonomic `View` / `Text` / `Button`
builders still live in `mkui-core`; they lower into the runtime tree via
a `LoweringRegistry` (extension-friendly, mirrors `WebRendererRegistry`).

### Does this supersede ADR 0001?

**No.** ADR 0001 says `mkui-core` is the contract crate. ADR 0005 says
the *implementation* of those contracts (storage, action plumbing, class
parser) lives one crate deeper. `mkui-core` continues to own the public
Rust API surface (Component trait, builders, headless logic, theme).
Codex round-7 Q1 explicitly ratified this — runtime is the
contract-implementation layer, not a replacement for the contract crate.

### Builder-as-sugar-on-handles

One storage model (`AppTree`), two construction frontends:

1. **Rust ergonomic builder** — `Mkui::new().child(View::new()...)`.
   `mkui-core::Mkui::child` lowers the builder into runtime nodes through
   the `LoweringRegistry`. The public Rust API is byte-unchanged from
   v0.4.x (showcase-common compiles unchanged — issue #51 §9).
2. **FFI handle API** — `mkui_app_view_child(app, parent, "class")` /
   `app.view_child(parent, class)`. C and Python mutate the runtime tree
   directly through opaque `NodeId` handles.

Both frontends produce byte-identical JSON snapshots (issue #51 §2).

### Hybrid action model

`ActionRegistry` owns the binding-agnostic action *id*; closure storage
lives wherever it can:

- Rust: `register_local(FnMut(&mut RuntimeCtx))` — stored in
  `Rc<RefCell<...>>` inside the registry.
- C: `mkui_app_register_callback(func, user_data)` — function pointer +
  void pointer stored in the C binding's parallel table keyed by
  `ActionId.index()`.
- Python: `app.register_callback(callable)` — `Py<PyAny>` stored on the
  Python binding's parallel table.

Renderers (web onclick, console Enter, wgpu mouse) fire by `ActionId`.
The binding looks the id up in its own table and calls the appropriate
form of callable.

### `ActionRegistry` is single-threaded

No `Send + Sync` bounds. Codex round-7 anti-pattern guard #12 — adding
thread-safety prematurely would force every binding to thread bounds
through closures that never cross threads. Add it back when a real
concurrent runtime arrives.

### Class parser location

`StyleClass` + `ResolvedStyle` live in `mkui-runtime`. Renderers consume
`ResolvedStyle` for typed layout/style decisions. Web may also forward
the raw `class.raw()` to the DOM as a Tailwind class string; parity tests
compare `ResolvedStyle`, not the raw forwarding.

The parser is a strict three-tier model:

- **Tier 1** — 43 utility classes (the showcase set; issue body's text
  says 39, the enumerated list contains 43 — we ship the enumerated list).
- **Tier 2** — `hover:*`, `sm:*`, `transition-colors` parse as documented
  no-ops. A `tier2_count` field surfaces in `ResolvedStyle` so parity
  tests can assert tolerance.
- **Tier 3** — anything else is a parse error with a helpful message that
  names the bad token and the tier system.

### JSON snapshot format

Compact serialisation via `serde_json::to_string`. Field order follows
the derived `Serialize` impl, not `BTreeMap` sort, so the schema mirrors
the typed shape. Children inline depth-first in declaration order.
Pretty-print exists for diagnostics; the parity gate uses the compact
form.

## Consequences

- **Sprint 4 dependency bumps land here.** PyO3 0.22 → 0.28.3 (unblocks
  Python 3.14, audit Phase 5 Task 24). cbindgen 0.26 → 0.29.2 (clears
  `atty` + `clap 3` + `bitflags 1` + `syn 1` transitive duplicates;
  prunes 3 of 4 advisory ignores).
- **`mkui-c` re-enters CI build-release + clippy.** The handle-based
  rewrite folds in `// SAFETY:` annotations on every `unsafe` block
  (audit Phase 1.1) and clears `clippy::not_unsafe_ptr_arg_deref` by
  design (opaque handles instead of inline pointer-deref).
- **`mkui-py` CI re-entry is gated on PyO3 0.28 + Python 3.14 working
  end-to-end on the CI image.** If the bump surfaces blocking link
  issues (macOS Python lookup, manylinux wheel), `mkui-py` stays
  excluded for one more sprint; the runtime work still ships.
- **Sprint 5's wgpu declarative bridge** (returning #50 scope) becomes
  "render the AppTree" instead of "rebuild parallel storage". The
  substrate is the prerequisite.
- **Sprint 6+ shadcn parity components** (Separator, Tabs, Checkbox, …)
  plug into the `NodeKind::Custom` extension slot. The Sprint 4
  `TestWidget` extension proof exercises the round-trip end-to-end.

## Alternatives considered

- **Absorb the AppTree into `mkui-core`.** Rejected because it would
  violate ADR 0001's "contract crate has no backend deps" stance —
  `serde_json` (for snapshots) is a substrate-level dep, not a contract.
- **One AppTree per backend.** Rejected — that's the architecture the
  issue body identifies as the root cause of every reported drift. Single
  substrate, one parse, one snapshot.
- **Send + Sync action registry on day one.** Rejected per Codex Q3
  anti-pattern guard; revisit when a real multithreaded runtime exists.
- **Class parser in `mkui-web` only.** Rejected — every renderer needs the
  typed projection eventually (wgpu HUD layout, console flex). Centralising
  the parser at the substrate layer is the only place the cost amortises.
