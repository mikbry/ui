# Architecture Decision Records

This directory tracks the load-bearing architectural decisions in mkui's
history. Each record (ADR) documents one decision: why it was made, what was
considered, and what the consequences are. New contributors and reviewers
should be able to understand the shape of the workspace by reading this index
and the four records below.

## Index

- [ADR 0001 — `mkui-core` as the contract crate](0001-mkui-core-as-contract-crate.md)
  — A single, domain-free contract crate that every backend depends on; the
  dependency graph is strictly one-way from bindings down to `mkui-core`.
- [ADR 0002 — `mkui-text` owns the stack (no external Rust text crates)](0002-mkui-text-own-the-stack.md)
  — Zero external text-stack dependencies; the bitmap prototype today, GPU
  outline rendering in future sprints, all owned in-tree.
- [ADR 0003 — `mkui-web` registry-based extension](0003-mkui-web-registry-based-extension.md)
  — A `TypeId`-keyed registry replaces the closed-set downcast, so downstream
  crates can register custom component renderers without patching `mkui-web`.
- [ADR 0004 — `mkui-wgpu` 2D HUD pipeline port](0004-mkui-wgpu-2d-hud-pipeline-port.md)
  — A bounded port of an upstream 2D HUD renderer (~600 lines) + a `winit`
  `ApplicationHandler` shell gives `mkui-wgpu` a working wgpu surface in one
  sprint.
- [ADR 0005 — `mkui-runtime` as the portable AppTree substrate](0005-mkui-runtime-portable-substrate.md)
  — A new `mkui-runtime` crate owns the arena-backed `AppTree`,
  generation-counter `NodeId` / `ActionId` handles, and the class parser
  every binding shares.
- [ADR 0006 — `mkui-wgpu` declarative bridge over `mkui-runtime::AppTree`](0006-wgpu-declarative-bridge.md)
  — `mkui-wgpu` walks the runtime tree into the existing tessellation
  pipeline; `WgpuRenderable` + `WgpuRendererRegistry` stay backend-local.
  The declarative `Mkui::new()?.child(...).run()` API is the documented
  primary path; `Mkui::with_scene` is a retained low-level raw-scene escape
  hatch (renderer tests, custom HUDs, headless tessellation demos, future
  direct-GPU experiments), not deprecated.

## ADR format conventions

Each ADR is one file, one decision, one page.

### File naming

`NNNN-short-kebab-case-name.md` — four-digit zero-padded sequence number,
followed by a short hyphenated descriptor. Numbers are append-only; a
superseded ADR keeps its number and updates its Status field to point at
the superseding ADR.

### Section structure

Every ADR has these sections, in this order:

- **Status** — `Accepted`, `Proposed`, `Superseded by ADR NNNN`, or
  `Deprecated`. Include the sprint and date when the decision was made.
- **Context** — what forces the decision: the situation, the constraints,
  the prior state. No "Decision" content here.
- **Decision** — the call that was made, stated decisively. Imperative
  voice: "Replace the closed-set downcast with `WebRendererRegistry`",
  not "We could replace it…".
- **Consequences** — what changes as a result. Both the wins and the
  costs. A future maintainer reading the Consequences should know what
  they are signing up for.
- **Alternatives considered** — at least one rejected alternative, with
  a one-paragraph reason for rejection. This is the section that prevents
  the same conversation from being re-litigated next sprint.

### When to write an ADR

- A decision changes the shape of the dependency graph or a crate boundary.
- A decision commits to or rejects a substrate choice (rendering, text,
  layout, async, etc.).
- A decision is non-obvious from the code — a future maintainer reading the
  diff would ask "why was this chosen over X?".

If a change is a routine bug fix, refactor, or feature addition that does
not change architecture, it does not need an ADR. The commit message and PR
description are enough.

### When NOT to write an ADR

- Every PR. ADRs are reserved for architectural decisions, not changelog
  entries.
- Decisions still under active debate. Use a draft proposal or an issue
  thread; promote to an ADR once accepted.
- Implementation detail captured by the code itself (a struct's field
  ordering, a function's error type, a test's assertion shape).
