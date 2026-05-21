# mkui ↔ shadcn/ui mapping

> **shadcn/ui is the naming, variant, and composition target for mkui — not the implementation target.** mkui's implementation target is its own immediate-mode wgpu architecture (per [`docs/research/mkui-text-state-of-the-art.md`](../research/mkui-text-state-of-the-art.md) and the existing `mkui-wgpu` crate). shadcn's DOM/CSS conventions (slots, `data-state` attributes, headless-vs-styled subcomponent splits) do not transfer directly; what transfers is the **catalog shape**: which primitive components exist, what variant names they expose, and how compositions are layered.
>
> **Why this exists:** when mkui ships an atom, its public name + variant enum + state model must match shadcn where shadcn has an equivalent. This makes mkui legible to downstream Rust UI consumers who already know the shadcn vocabulary, and it keeps mkui's surface area from drifting into application-specific shapes. The reverse is also true: where shadcn has no equivalent (it's a web-DOM library, not a desktop framework), mkui ships its own component under a generic name aligned with desktop UI conventions, not under a product-specific name.
>
> **What this document is NOT:** it is not a list of every component any consumer will need. Application-specific components (e.g. Miky's StatePill, RoleBadge, TierTag, row-builders, app shell) live in **the application's own UI layer**, downstream of mkui. mkui ships generics; the app composes them. Per the Codex review (2026-05-21), keeping this boundary strict is what makes mkui usable as an internal open framework across multiple projects rather than a Miky-shaped component dump.
>
> **References:**
> - shadcn/ui canonical catalog: <https://ui.shadcn.com/docs/components>
> - shadcn variants pattern (cva): <https://cva.style>
> - `mkui-wgpu`'s existing implementation: `/Users/mik/dev/mikbry/ui/crates/mkui-wgpu/src/{components.rs,theme.rs,builder.rs}` — already exposes shadcn-shaped `ButtonVariant`/`ButtonSize`/`ButtonState`/`TextVariant` enums from the upstreaming work in PR #12.
> - StoneSketch's existing convergence notes: `/Users/mik/dev/astoneer/stonesketch/docs/gui.md` lines 130-178 — the reference mapping that originally informed `mkui-wgpu`'s component surface.

---

## Mapping principles

These are binding for any new mkui component PR.

### 1. Use shadcn's name when shadcn has an equivalent

If a component's role matches a shadcn component, the mkui API uses the shadcn name as primary. No aliasing for design-language convenience (e.g. don't ship `Chip` as a public alias for `Badge` — design docs can keep their preferred language, the code uses `Badge` only).

### 2. Generic shadcn variants only on generic mkui APIs

`Badge` variants in mkui-wgpu are limited to shadcn's set: `Default | Destructive | Outline | Secondary`. Product-specific variants (warm-amber attention states, repo-role tags, tier classifications) do **not** land on the generic enum — they live in the downstream application's adapter layer (e.g. a `<consumer>-ui` crate that depends on mkui).

### 3. Variant *sizes* and *states* track shadcn's cva pattern

mkui's enums mirror shadcn's: `Size::{Default, Sm, Lg, Icon}`, `State::{Default, Hover, Active, Disabled}` where the component supports those states. The cva-style resolver pattern (variant + size + state → concrete style) is already implemented in `mkui-wgpu/src/theme.rs` via `HudTheme::button_style(variant, size, state)`.

### 4. Compositions are *not* new components

Where a desired UI element is a composition of mkui primitives (e.g. a card that wraps a state pill + body + meta columns for a specific application's data row), it does **not** ship as a new mkui component. The application composes from mkui primitives directly, or layers in its own composition crate. mkui's `UiBuilder<T>` (in `mkui-wgpu/src/builder.rs`) exposes generic row primitives (`heading`, `text`, `readonly_row`, `number_row`, `list_row`); downstream apps extend `UiBuilder` via traits if they need higher-level row shapes.

### 5. DOM/CSS-isms don't transfer

shadcn relies on web idioms — children-as-slots, `data-state` attributes, separate `Trigger` / `Content` subcomponents for radix-style headless primitives. mkui is immediate-mode and renders scene primitives. The transfer is at the **vocabulary** layer (names + variants + composition shape), not the API mechanics. Example: shadcn `Tabs` has `Tabs / TabsList / TabsTrigger / TabsContent` — mkui's `tabs()` builder accepts `&[(label, content_fn)]` and emits the equivalent scene. The naming and the variant model match; the call shape doesn't.

### 6. Where shadcn has no equivalent — ship under a generic desktop name

shadcn is a web-DOM library. It doesn't have desktop chrome primitives (Titlebar, StatusBar, ResizeHandle), nor status indicators (Dot), nor keyboard glyph (Kbd). mkui ships these under their conventional desktop names. They are not product-specific — they are generic desktop UI primitives that any consumer will want.

---

## Atoms — mkui's first-class generic catalog

The components below ship in `mkui-wgpu::components::*`. They are application-agnostic.

### Badge

| Property | shadcn equivalent | mkui implementation |
|---|---|---|
| **Name** | `Badge` | `mkui_wgpu::components::badge` |
| **Variants** | `default \| destructive \| outline \| secondary` | `BadgeVariant::{Default, Destructive, Outline, Secondary}` |
| **Sizes** | one size (shadcn) | `BadgeSize::{Default, Sm}` (mkui adds a smaller size for dense tables) |
| **States** | none (badges are non-interactive) | none |
| **Notes** | The shadcn variants are the **complete** generic set in mkui. Application-specific variants (warm-amber attention badges, role-color badges, tier-color badges) live in downstream app crates as wrappers around `Badge` with custom theme tokens, not as new mkui variants. |

### Dot

| Property | shadcn equivalent | mkui implementation |
|---|---|---|
| **Name** | (none — shadcn has no status-dot primitive) | `mkui_wgpu::components::dot` |
| **Variants** | — | `DotVariant::{Ok, Warn, Danger, Neutral}` — status-color tokens, not product semantics. Plus a separate `halo: bool` modifier for the higher-emphasis "attention halo" geometry common in desktop UI. |
| **Sizes** | — | `DotSize::{Sm, Md}` (6px / 8px diameter) |
| **States** | — | Optional `DotAnimation::{None, Pulse, PulseUrgent, Spin}` — generic motion primitives, consumer decides semantic |
| **Notes** | The variant names are status-color references (`ok`/`warn`/`danger`/`neutral`), not application semantics. The `halo` and animation flags are visual primitives; what they *mean* is the consumer's choice. |

### Button

| Property | shadcn equivalent | mkui implementation |
|---|---|---|
| **Name** | `Button` | `mkui_wgpu::components::button` *(already shipped — PR #12)* |
| **Variants** | `default \| destructive \| outline \| secondary \| ghost \| link` | `ButtonVariant::{Default, Destructive, Outline, Secondary, Ghost, Link}` — matches shadcn 1:1 |
| **Sizes** | `default \| sm \| lg \| icon` | `ButtonSize::{Default, Sm, Lg, Icon}` |
| **States** | `default \| hover \| active \| disabled` | `ButtonState::{Idle, Active}` *(extend with `Disabled` when needed)* |
| **Notes** | Already shadcn-aligned post-Sprint-1. No structural changes needed. |

### Avatar

| Property | shadcn equivalent | mkui implementation |
|---|---|---|
| **Name** | `Avatar` | `mkui_wgpu::components::avatar` |
| **Sources** | image, image + fallback, fallback-only | `AvatarSource::{Image(uri), Initials(text), Gradient { hue_a, hue_b }}` |
| **Sizes** | one size (shadcn re-styles per use) | `AvatarSize::{Sm, Md, Lg}` |
| **Notes** | Direct mapping. Gradient variant is generic (any hue pair), not product-specific. |

### Kbd

| Property | shadcn equivalent | mkui implementation |
|---|---|---|
| **Name** | semantic `<kbd>` element (shadcn uses it in command-palette examples) | `mkui_wgpu::components::kbd` |
| **Variants** | (none) | — |
| **Notes** | Generic desktop UI primitive. Mono font, small label-caps text, faint background, 0.5px border — standard `<kbd>` visual treatment. |

---

## Inputs — mkui's input catalog

### Input

| Property | shadcn equivalent | mkui implementation |
|---|---|---|
| **Name** | `Input` | `mkui_wgpu::components::input` |
| **Variants** | one variant (shadcn) | `InputVariant::{Default, Search}` (search adds a leading magnifier icon + optional trailing Kbd hint) |
| **States** | `default \| focus \| disabled \| invalid` | All four |
| **Slot APIs** | shadcn uses children-as-slots for icons/addons | mkui uses explicit `leading_icon: Option<IconId>` + `trailing: Option<TrailingContent>` parameters since immediate-mode doesn't have JSX children |
| **Notes** | Composition of icon + input + addon happens via parameters in mkui, not via children-as-slots. The downstream call shape is one function call, not a nested tree. |

### Toggle / ToggleGroup

| Property | shadcn equivalent | mkui implementation |
|---|---|---|
| **Name** | `Toggle` (single button), `ToggleGroup` (mutually exclusive set) | `mkui_wgpu::components::toggle` + `toggle_group` |
| **Variants** | `default \| outline` | Same |
| **States** | `data-state=on\|off` | `ToggleState::{Off, On}` |
| **Notes** | `ToggleGroup` with `type="single"` is the canonical "segmented control" — the sliding-thumb visual is a motion variant, not a separate component. |

### Tabs

| Property | shadcn equivalent | mkui implementation |
|---|---|---|
| **Name** | `Tabs` | `mkui_wgpu::components::tabs` |
| **Variants** | one default look (shadcn re-styles per use) | `TabsVariant::{Default, Pill}` (pill is the rounded segmented look common in macOS) |
| **Sizes** | (none — shadcn relies on consumer styling) | `TabsSize::{Sm, Md}` |
| **Subcomponent split** | `Tabs / TabsList / TabsTrigger / TabsContent` | mkui exposes a single `tabs()` builder accepting `&[(label, content_fn)]` — the split is internal to the builder, not part of the public API |
| **Notes** | The visual conventions match; the call shape is mkui-native immediate-mode rather than shadcn's nested-element pattern. |

---

## Structural — mkui's app-shell catalog

These are generic desktop UI primitives, not application-specific compositions.

### Window

shadcn doesn't ship a `Window` primitive (it targets web). mkui's `window()` is the top-level container that owns the wgpu surface and dispatches events. The implementation lives in `mkui-wgpu::app::App` (the winit `ApplicationHandler` shell from Sprint 2's renderer port).

### Titlebar

Generic top-bar primitive: leading area (left-aligned controls), trailing area (right-aligned controls), centered overflow region. shadcn has no equivalent. Implemented as a layout primitive in `mkui-wgpu::components::titlebar`.

### Sidebar

shadcn shipped a `Sidebar` component in late 2024 (<https://ui.shadcn.com/docs/components/sidebar>). Direct mapping: mkui's `sidebar()` adopts the shadcn API shape — `SidebarVariant::{Sidebar, Floating, Inset}`, subcomponents (head / content / foot / group / menu) exposed as named slots in the builder.

### StatusBar

Generic VS-Code-style bottom bar: left-aligned segments + right-aligned segments + optional center. Each segment is interactive. shadcn has no equivalent. Implemented as a layout primitive.

### Popover + DropdownMenu

Direct shadcn mapping. Both are floating-content primitives anchored to a trigger.

### Card

Direct shadcn mapping. Card variants match shadcn's. Subcomponents (`Header`, `Title`, `Description`, `Content`, `Footer`) are exposed via builder slots.

### Alert

Direct shadcn mapping. Variants: `default | destructive`. Generic-purpose alert/banner primitive.

### Progress

Direct shadcn mapping. Segmented and continuous variants.

### ScrollArea

Direct shadcn mapping. Already prototyped in `mkui-wgpu::components::scroll_area`.

### Separator

Direct shadcn mapping. Horizontal and vertical rules.

### ResizeHandle

Generic desktop primitive (drag-strip on panel edges). shadcn has no equivalent. Implemented as a hit-region primitive.

---

## Components mkui does NOT ship

These are intentionally **out of scope** for mkui's catalog. They are application-specific compositions that live in the consumer's own UI layer (a `<consumer>-ui` crate that depends on mkui).

| Component class | Why out of scope | Where it lives |
|---|---|---|
| Product-specific atoms (state pills encoding application state machines, role tags for application domains, tier classifiers, ledger markers) | These compose generic mkui primitives (`Badge`, `Dot`, `Card`) with application-specific theme tokens and state mappings | Consumer app's UI crate |
| Row builders for specific data types (PR rows, issue rows, agent cards, audit rows) | These are domain compositions, not generic primitives | Consumer app's UI crate, optionally as `UiBuilder` extension traits |
| Application-specific composites (status banners with custom dismiss behavior, batch progress headers, gate progression strips) | These bundle multiple generic primitives + app-specific timing/state | Consumer app's UI crate |
| Application-specific layout primitives (right-rail action panels, action-routing surfaces) | These encode application information hierarchy, not UI conventions | Consumer app's UI crate |

For Miky's catalog specifically, the mapping work (Miky design → mkui primitives + miky-ui composition layer) is tracked in [`mikbry/miky-internal#6`](https://github.com/mikbry/miky-internal/issues/6). Future external consumers will surface their own equivalents; the boundary discipline is the same.

---

## Theme tokens

mkui-core's `ThemeTokens` struct (already in `mkui-wgpu/src/theme.rs`) carries shadcn-aligned generic tokens:

- `primary`, `primary-foreground`
- `secondary`, `secondary-foreground`
- `muted`, `muted-foreground`
- `accent`, `accent-foreground`
- `destructive`, `destructive-foreground`
- `border`, `input`, `ring`
- `background`, `foreground`
- `surface`, `card`, `popover`

Consumers extend this struct with their own theme extensions when they need product-specific tokens (warm-amber attention containers, tier color scales, density variants, brand-specific gradient stops). The extension pattern is via composition, not via inheritance — the consumer's app holds both the generic `ThemeTokens` and its own extension struct, and passes both to the relevant components.

---

## Open questions

These are unresolved and benefit from external review. Codex round-N candidates.

1. **`Card` subcomponent surface.** shadcn exposes `Card / CardHeader / CardTitle / CardDescription / CardContent / CardFooter`. Should mkui's `card()` builder accept all six as named slots, or restrict to `header / body / footer` for simplicity? The shadcn count feels generous for an immediate-mode framework.

2. **`Sidebar` adoption depth.** shadcn's `Sidebar` API includes `SidebarMenu`, `SidebarGroup`, `SidebarMenuItem`, `SidebarMenuButton`, plus `useSidebar()` hooks for state. The first mkui implementation can't ship all of that. What's the minimal subset that's still "shadcn-aligned" rather than "shadcn-inspired but different"?

3. **`Toast` / `Sonner`.** Useful generic primitive but requires a portal-style rendering layer + timer state. Defer to Sprint 5+ unless a consumer needs it sooner.

4. **`Dialog` / `Drawer`.** Same portal + state-management story as `Toast`. Generic, useful, but architecturally weighty. Defer.

5. **`Form` / `Field` / validation.** shadcn's form primitives wrap react-hook-form. mkui has no equivalent state-management story yet. Validation is a downstream concern; mkui should ship `Input`/`Toggle`/`Tabs` with state-handling parameters and let consumers add validation in their own code.

---

## Counts

| Category | Count | Components |
|---|---|---|
| Generic atoms (shadcn-aligned) | 5 | `Badge`, `Dot`, `Button`, `Avatar`, `Kbd` |
| Generic inputs | 4 | `Input`, `Toggle`, `ToggleGroup`, `Tabs` |
| Generic structural | 11 | `Window`, `Titlebar`, `Sidebar`, `StatusBar`, `Popover`, `DropdownMenu`, `Card`, `Alert`, `Progress`, `ScrollArea`, `Separator`, `ResizeHandle` |

**Total mkui catalog: 20 first-class components.** Sprint 2 ships 2 (Badge + Dot — see [`docs/sprint-2-plan.md`](../sprint-2-plan.md)); the remainder ships across Sprints 3-5.

Compositions, row-builders, app-shell layouts for specific applications, and product-specific atoms are NOT in mkui's catalog. They live in consumer crates.

---

**Last updated:** 2026-05-21. Rewritten after Codex review (2026-05-21) flagged the prior version's variant pollution + row-builder boundary violations. The Miky-specific mapping work that was incorrectly drafted in the prior version is now tracked in [`mikbry/miky-internal#6`](https://github.com/mikbry/miky-internal/issues/6).
