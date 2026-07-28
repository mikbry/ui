# Dame du lac rubric — mkui Slug rendering

**Version:** v1.2.1 (2026-07-28, amendment 1) — awaits operator ratification. Amends v1.2 (Phase 2 § "No thin-gap regressions" now excludes texels byte-identical to reference; per Phase 2 chevalier BLOCKED.md `oracle_ambiguity`). Supersedes v1.0/v1.1 drafts at `strategy/goals/mkui-slug-rewrite/dame-rubric.md`.
**Referenced by:** [`CHARTER.md`](CHARTER.md) § `verification_oracle.dame_invocation`
**Immutable during mission** — see CHARTER § Immutability + § Amendment protocol.

## What this document is

Dame du lac's phase-boundary decision rubric. Dame reads this rubric + the ratified reference adapter (`reference-harness/`) + the frozen Codex plan (`codex-8-step-plan.md`), applies each criterion verbatim to chevalier's PR, returns **BLESS** (all checks pass) or **REFORGE** (specific failing checks + provenance).

Dame **cannot** add criteria, reinterpret criteria, or judge outside the rubric. If a criterion is ambiguous, dame returns REFORGE with `blocked_reason: oracle_ambiguity` and requests operator amendment.

Chevalier reads this too — informs self-verification, allowing chevalier to pre-check before dispatching dame.

## Reference oracle artifacts (dame reads, does not reinterpret)

### Ratified verification adapter (primary)

- **`reference-harness/`** at merge SHA `9f76af3` (PR #158 merged 2026-07-28) — the WGSL port of pinned upstream Slug HLSL. Contains 24 known-good goldens (8 glyphs × 3 DPI), 1 seeded-mutant proof (horizontal-root sign flip), per-line HLSL provenance in `PROVENANCE.md`, upstream pins in `UPSTREAM.md`, `Cargo.toml` + `src/main.rs` for reproducibility.
- Adapter's `PROVENANCE.md` § Seeded-mutant sensitivity result — the calibration reference for this rubric's numeric thresholds.

### Algorithm cross-reference (secondary)

- **JCGT paper** (Lengyel 2017): https://jcgt.org/published/0006/02/02/ — canonical algorithm description. Referenced for correctness cross-check when adapter's implementation is ambiguous.
- **Official Slug pixel shader** (HLSL) at pinned SHA `be3c13eb7d63f9e8aa5c583e42d92c374cb91d98`: https://github.com/EricLengyel/Slug/blob/be3c13eb7d63f9e8aa5c583e42d92c374cb91d98/SlugPixelShader.hlsl — vendored via adapter; consult only if the adapter's provenance annotations don't resolve a specific pixel-level question.
- **"A Decade of Slug" retrospective**: https://terathon.com/blog/decade-slug.html — algorithmic evolution + modern discipline (dynamic dilation, no supersampling). Consulted for Phase 2 dilation cross-check.

### Frozen execution plan

- **`codex-8-step-plan.md`** — the frozen Codex 8-step plan (was `#157` body at ratification time). This is what chevalier is implementing; dame verifies chevalier's deliverable against this plan's per-step scope.

## Reference golden generation method

**Dame regenerates goldens via the ratified adapter at each phase check.** Method:

```bash
# From the repo root:
cd docs/chevalier/mkui-slug-rewrite/reference-harness/
# Ensure docker + Lavapipe stack per PROVENANCE.md § Toolchain:
docker run --rm -v "$PWD:/w" -w /w \
  rust@sha256:948f9b08a66e7fe01b03a98ef1c7568292e07ec2e4fe90d88c07bb14563c84ff \
  bash -c "cd /w && cargo run --release -- --glyph <G> --dpi <D> --output /tmp/golden.png"
```

Storage-mode selection (per miky #158 N1):
- Default: `--storage rgba32float` (adapter's ratified capture mode, matching the pinned known-good PNG SHA-256s in `PROVENANCE.md`)
- Optional: `--storage rgba8unorm-16bit` for comparison when chevalier's implementation uses upstream README's compact 16-bit format. Dame runs BOTH storage modes and picks the mode matching chevalier's implementation; both modes must satisfy the rubric.

**Docker availability check** (per miky #158 N2): dame verifies docker + the pinned image are accessible before invoking the adapter. If not: return REFORGE with `blocked_reason: docker_unavailable`; do NOT silently fall back to non-hermetic capture.

**Adapter drift check**: dame recaptures the known-good `H_1x.png` via the adapter and compares to the pinned SHA-256 in `PROVENANCE.md`. Mismatch → return REFORGE with `blocked_reason: adapter_drift` (indicates toolchain drift; operator investigates).

## Threshold calibration — from adapter's own sensitivity test

The adapter's PROVENANCE.md § Seeded-mutant sensitivity result establishes empirically:
- **Detection floor**: max per-channel Δ > 8/255 (~3% of full range) reliably detects a real algorithm defect
- **Defect signal shape**: seeded mutant produced 76 differing pixels in 1/128 columns (column x=25), max channel Δ = 153/255 (~60%)

Rubric thresholds are calibrated to this floor — tight enough to catch real divergence, loose enough to accommodate numerical noise from cross-provider WGSL compilation:

- **Per-channel Δ BLESS threshold**: max per-channel Δ ≤ **4/255** (~half the adapter's detection floor)
- **Differing-pixel BLESS threshold**: ≤ **10** pixels per glyph per DPI (the adapter's own known-good rerun should produce 0; allow up to 10 for wgpu / driver numerical variance)
- **SSIM BLESS threshold**: ≥ **0.995** (near-identical shader outputs on same-algorithm implementation)

Failure of ANY of the three above = REFORGE.

## Per-phase BLESS criteria

Each phase's criteria labels:
- **(S)** structural — grep / presence / absence check
- **(N)** numeric — threshold on measured value
- **(T)** test-pass — existing or new test harness

### Phase 1 — Codex 8-step-plan steps 1+2+3 (dual-ray coverage + fwidth AA)

**Scope:** upload vertical bands + vertical curve indices to GPU; port official H+V weighted coverage with root eligibility rules from the adapter's `reference.wgsl`; replace `scale_px_per_unit` AA width with `fwidth()` derivatives.

BLESS if ALL of the following pass:

- [ ] **(S) Vertical band upload present**: `crates/mkui-vector2d-wgpu/src/lib.rs` uploads vertical bands. Grep: at least one of `vertical_bands`, `pack_vertical`, or a code path that binds vertical band texture in addition to horizontal. Absent → REFORGE with line references.
- [ ] **(S) Vertical curve index upload present**: `crates/mkui-vector2d-wgpu/src/lib.rs` uploads vertical curve indices matching the vertical bands. Analogous structure to the horizontal path.
- [ ] **(S) Dual-ray coverage in shader**: `crates/mkui-vector2d-wgpu/src/slug.wgsl` fragment shader computes BOTH horizontal AND vertical coverage. Compare to adapter's `reference.wgsl` § `solve_horiz_poly` (lines 107-120) + § `solve_vert_poly` (lines 122-135). Structural equivalence required (WGSL function names may differ; two axis-distinct root-solver call sites required).
- [ ] **(S) Weighted combination present**: chevalier's shader combines H+V coverage per JCGT §4.2 / adapter's `reference.wgsl` § `slug_render` (lines 151-210). Grep for the weighted-combination formula analogous to the adapter's implementation.
- [ ] **(S) Root eligibility check ported**: shader's root solver rejects roots per JCGT §3.3 / adapter's `reference.wgsl` § `calc_root_code` (lines 98-105, using the `0x2E74` root-eligibility code). OLD code (previous `slug.wgsl:114` per Codex R0 review) accepted roots in `[0,1)` directly — that block must NOT survive.
- [ ] **(S) `fwidth()` derivative AA width**: shader uses `fwidth(in.font_pos)` (or `dpdx()` + `dpdy()` equivalent) for antialiasing width. Grep: at least one call to `fwidth` on the interpolated glyph coordinate.
- [ ] **(S) `scale_px_per_unit` no longer used as AA width**: grep the shader for any remaining reference to `scale_px_per_unit` in an AA-width context. Zero matches required. Uses of `scale_px_per_unit` for other purposes (e.g. glyph position scaling in vertex stage) are allowed.
- [ ] **(N) Adapter-comparison per-channel Δ ≤ 4/255**: for each of 8 glyphs × 3 DPI = 24 comparisons, chevalier's rendered output vs adapter's known-good golden (same storage mode) has max per-channel Δ ≤ 4/255. Comparison method: byte-level per-pixel diff on RGBA. Any comparison exceeding → REFORGE with specific glyph + DPI + measured Δ + differing pixel count.
- [ ] **(N) Differing pixel count ≤ 10** per comparison (chevalier vs adapter, same storage mode).
- [ ] **(N) SSIM ≥ 0.995** per comparison.
- [ ] **(N) Coverage bounded in pre-clamp float buffer**: dame samples chevalier's fragment shader's pre-clamp float coverage value (via debug binding or `capture_texture` on an unclamped intermediate). All samples MUST be in `[-0.001, 1.001]` (tiny numerical slack allowed). Post-clamp output PNG bytes are tautologically in `[0, 255]` — this check runs on the pre-clamp float buffer, not the PNG. If chevalier doesn't expose an unclamped intermediate, this criterion is documented as `dame_infrastructure_gap`.
- [ ] **(T) `cargo test -p mkui-vector2d-wgpu` passes** — 100% of existing tests + any new tests chevalier added.
- [ ] **(T) `cargo test -p mkui-vector2d` passes** — no regressions in the CPU-side substrate.
- [ ] **(T) `cargo test -p mkui-wgpu` passes** — no regressions in the wgpu integration.

REFORGE response structure — see § Dame's decision output format.

### Phase 2 — Codex 8-step-plan steps 4+5 (bounded dilation + band overlap epsilon)

**Scope (narrowed per CHARTER v1.2):** half-physical-pixel dilation for the expanded quad — bounded 2D case, NOT full transform-aware dynamic dilation from the retrospective. Band overlap epsilon (~1/1024 em) to close floating-point gaps.

BLESS if ALL of the following pass:

- [ ] **(S) Bounded dilation replaces constant 1.5-logical-px expansion**: chevalier's quad-expansion code uses `fwidth()`-derived OR `pick_sample_count`-aware dilation for the 2D case. Grep for the previous `1.5` constant in a dilation context — must be replaced with derivative-based OR removed and documented. Reference: adapter's `reference.wgsl` § `slug_dilate` (lines 52-79); "A Decade of Slug" retrospective, section "Dynamic Dilation".
- [ ] **(S) Dilation is 2D-bounded, NOT transform-aware**: chevalier's dilation implementation does NOT include per-render MVP/viewport-derived transform dilation (that's the retrospective's full scope; deliberately out-of-scope for this mission per CHARTER Phase 2 scope note). Grep for MVP/viewport-related terms in the dilation code path; if present, that's over-scope → REFORGE with `blocked_reason: over_scope`.
- [ ] **(N) Dilation width ≈ 0.5 * physical_pixel_size at each DPI**: dame renders test glyph `plus.slug` at 1×, 1.5×, 2× and inspects the emitted quad extent in NDC. Measured dilation MUST be within ε=0.05 physical pixels of `0.5 * physical_pixel_size` at each DPI. Deviation reports actual dilation + expected.
- [ ] **(S) Band overlap epsilon present in `crates/mkui-vector2d/src/slug.rs`**: CPU band builder overlaps adjacent bands by a small epsilon. Grep for the overlap arithmetic (previous `slug.rs:431` used exact inclusive overlap per Codex R0 review).
- [ ] **(N) Band overlap epsilon = 1/1024 em**: the overlap constant, when normalized to em space using `units_per_em`, equals `1.0 / 1024.0` within ε=1e-7. If expressed in font design units, dame validates via unit conversion. Reference: adapter's `PROVENANCE.md` § Fixture provenance ("upstream README's `1/1024` em overlap").
- [ ] **(S) Bands normalized to em space OR `units_per_em` provided**: grep for `units_per_em` in the CPU-side code path.
- [ ] **(N) Adapter-comparison per-channel Δ ≤ 4/255**: rerun Phase 1's per-glyph-per-DPI comparison AFTER Phase 2 changes. All 24 comparisons must still pass with tightened threshold — Phase 2 shouldn't loosen Phase 1's fidelity.
- [ ] **(N) No thin-gap regressions** (amended v1.2.1 per Phase 2 `oracle_ambiguity` block, 2026-07-28): dame renders `o` and `g` glyphs (curve-heavy) at 3 DPIs. Pixel-scan for zero-alpha pixels bordered by ≥50% alpha on both sides (thin-gap signature). Zero such pixels required, **EXCLUDING** texels that are byte-identical to the ratified reference-harness golden at the same coordinate. Rationale for the exception: the ratified reference oracle itself produces this pattern at tight curve intersections (empirically at `g_2x` texel (157, 156), neighbours 206/248 in the ratified adapter's own output) — this is ordinary analytic antialiasing, not a defect. Per Codex R1 P0 #1 + Finding L: a "regression" is definitionally a divergence from the ratified oracle, not an inherent property of the oracle's own output. Any thin-gap texel that byte-matches the reference is by construction NOT a regression. REFORGE cites specific coordinates + confirms the mkui-vs-reference delta at each cited coordinate (a REAL regression has both the thin-gap signature AND Δ > 0 vs reference).
- [ ] **(T) `cargo test -p mkui-vector2d` passes**.
- [ ] **(T) `cargo test -p mkui-vector2d-wgpu` passes**.
- [ ] **(N) Phase 1 SSIM criteria still hold**: re-run Phase 1's SSIM ≥ 0.995 measurement on all 24 comparisons. Regression → REFORGE.

### Phase 3 — Codex 8-step-plan steps 6+7 (cap-height snap + DPI golden-image tests)

**Scope:** snap baseline / cap-height to physical pixel grid for small text; golden-image regression tests at 1×/1.5×/2× DPI in the existing `gpu-offscreen` lane.

BLESS if ALL of the following pass:

- [ ] **(S) Baseline / cap-height snap present**: `crates/mkui-wgpu/src/app.rs` (or wherever glyph positioning sits) rounds baseline Y-coordinate to the physical pixel grid at layout time. Grep for `round()` / `floor()` / `ceil()` applied to a Y-coordinate derived from font metrics + DPI.
- [ ] **(S) Snap conditioned on small-text threshold**: snap applies below a documented cap-height threshold (per Codex plan step 6 "for small UI text"). Grep for a size check (e.g. `if cap_height_px < 16.0`). Above threshold, snap MUST NOT apply.
- [ ] **(N) Snap piecewise-constancy within quantization cells (revised per Codex R1 defect fix)**: dame renders `A` glyph at 12px cap-height at 100 sub-pixel Y-offsets uniformly spaced across one physical-pixel period `[0.0, 1.0)`. Renders MUST group into piecewise-constant cells matching the snap function's expected transitions:
  - For nearest-pixel snap: 2 cells (offsets `[0.0, 0.5)` → cell A; `[0.5, 1.0)` → cell B)
  - Within each cell: SSIM = 1.0 pairwise (byte-identical)
  - Cell boundary: expected 1-pixel transition in glyph position
  If chevalier uses a different snap function (e.g. floor-to-integer, cap-height-aware), rubric adjusts to that function's expected cell shape — chevalier documents snap function in `docs/architecture/N-cap-height-snap.md` and dame verifies cells match documented spec.
- [ ] **(S) Golden-image tests exist for 3 DPI scales**: `crates/mkui-wgpu/tests/goldens/` (or equivalent gpu-tests location) contains golden-image tests at 1×, 1.5×, 2× DPI. Grep for test names or module paths matching the DPI scales.
- [ ] **(T) Golden-image tests pass in `gpu-offscreen` job**: chevalier's PR CI shows the new golden tests green under VVL. Dame verifies via GitHub API check-suite results.
- [ ] **(N) Chevalier's committed goldens match adapter-regenerated goldens**: dame regenerates via adapter at each DPI + compares to chevalier's committed goldens in `crates/mkui-wgpu/tests/goldens/`. Same per-channel Δ ≤ 4/255, differing pixels ≤ 10, SSIM ≥ 0.995 as Phase 1.
- [ ] **(T) All prior-phase tests still pass** (Phase 1 + Phase 2 criteria re-checked, no regression).

### Phase 4 — Codex 8-step-plan step 8 (Abel/label routing decision)

**Scope:** route the demo label ("Abel via Slug, label via bitmap") through SFNT/Slug too, OR explicitly restrict bitmap to integer scales + device-pixel snapping. Scope-decision-first — chevalier documents which choice + rationale in `docs/architecture/N-bitmap-vs-slug-labels.md` before implementing.

Chevalier chooses variant A OR variant B. Dame verifies the chosen variant's criteria; the choice itself is not judged.

**Variant A — route label through SFNT/Slug:**

BLESS if ALL of the following pass:

- [ ] **(S) Bitmap-lane label rendering removed or gated**: the label text in `examples/text/` (or wherever the demo lives) uses the SFNT/Slug path OR is behind an opt-in flag. Grep for label-rendering code path.
- [ ] **(S) SFNT-through-Slug path present**: layout code invokes SFNT loader → Slug curve extraction → GPU submission via the same lane as the title glyphs. Grep for `SlugGlyph::from_sfnt` or equivalent bridge in `crates/mkui-text/`.
- [ ] **(N) SSIM comparison per size**: dame renders the label text at 12px via adapter (with adapter's `plus` / `pipe` used as calibration fixtures for that size) and compares chevalier's 12px render. SSIM ≥ 0.99 (looser than Phase 1's 0.995 since small-text rendering is inherently lossier). SAME comparison for 48px (using 48px reference goldens). Each size compared to its OWN corresponding reference render, NOT cross-size (per Codex R1 defect fix).
- [ ] **(N) Adapter-comparison per-channel Δ ≤ 6/255**, differing pixels ≤ 20 (loosened by 1.5× from Phase 1 for the small-text case).
- [ ] **(T) `cargo test -p mkui-text` passes** — new label-routing test coverage.

**Variant B — bitmap restricted to integer scales:**

BLESS if ALL of the following pass:

- [ ] **(S) Bitmap font restricted to integer scales**: bitmap rendering code path explicitly rejects OR clamps to nearest integer for non-integer scale values. Grep for scale-clamping logic + explicit rejection assertion.
- [ ] **(S) Device-pixel snapping applied**: bitmap output positions are snapped to device-pixel grid at emit time.
- [ ] **(N) Sub-pixel-invariance**: dame renders the bitmap label at 100 sub-pixel offsets (per Phase 3's snap-test logic). Piecewise-constancy per snap-function's expected cell shape (same math as Phase 3).
- [ ] **(T) `cargo test -p mkui-text` passes** — new bitmap-restriction test coverage.

Chevalier documents the variant choice + rationale in `docs/architecture/N-bitmap-vs-slug-labels.md` (created within Phase 4 YOLO scope) and cites it in the Phase 4 PR body.

**Phase 4 MERGE GATE (per Codex R1 P1 #6):** dame BLESS does NOT authorize chevalier to merge Phase 4 PR. Chevalier posts `COMPLETION.md` with Phase 4 PR OPEN + dame-BLESSED. Operator's visual smoke test on the 3 examples is the final merge gate. See CHARTER § Interaction points Point 2.

## Cross-phase invariants (checked at every phase boundary)

- [ ] **(T) `cargo fmt --check`** clean.
- [ ] **(T) `cargo clippy --workspace --exclude console-showcase --exclude web-showcase --exclude native-showcase --all-targets --locked -- -D warnings`** clean. (Excludes mirror CI per `mkui-ci-clippy-version-skew` memory pattern.)
- [ ] **(S) CHANGELOG.md `[Unreleased]` entry added for this phase's PR** — grep for a new `- **` bullet mentioning the phase's scope + issue link.
- [ ] **(S) No changes outside declared YOLO scope**: dame runs `git diff --name-only <prev-phase-tip>..HEAD` and asserts every changed file is in one of CHARTER § YOLO scope's Scope directories. Any out-of-scope changes → REFORGE.
- [ ] **(S) Ratified adapter unchanged since ratification**: dame runs `git log docs/chevalier/mkui-slug-rewrite/reference-harness/` and asserts zero commits from the chevalier since the adapter ratification SHA `9f76af3`. Adapter modification by chevalier = REFORGE with `blocked_reason: adapter_tampered`.
- [ ] **(S) Mission-input docs unchanged since ratification**: dame runs `git log docs/chevalier/mkui-slug-rewrite/{CHARTER,dame-rubric,codex-8-step-plan}.md` and asserts zero commits from chevalier since the mission-ratification SHA (this PR's merge commit). Tampering = REFORGE with `blocked_reason: rubric_tampered`.
- [ ] **(S) PR labels present**: `chevalier` + `mission:mkui-slug-rewrite` on the PR.
- [ ] **(T) VVL green on `gpu-offscreen` job**: chevalier's PR CI has the Vulkan validation layers active + passing (per #154 CI substrate). Dame verifies via check-suite name grep for `VK_LAYER_KHRONOS_validation` in job stdout.
- [ ] **(T) Docker + Lavapipe stack reachable** (per miky #158 N2): dame runs `docker info` + `docker pull rust@sha256:948f9b08a66e7fe01b03a98ef1c7568292e07ec2e4fe90d88c07bb14563c84ff` before adapter invocation. Failure → REFORGE with `blocked_reason: docker_unavailable`.

## Dame's own preflight sensitivity test (per miky #158 N3)

Before the first mission phase (chevalier's Phase 1), dame runs its own preflight sensitivity test to confirm the rubric can detect defects beyond the adapter's single committed mutant. Preflight fires ONCE at mission start (after CHARTER ratification, before Phase 1 dispatch); results captured in `docs/chevalier/mkui-slug-rewrite/dame-preflight-report.md`.

Preflight mutants (dame authors ephemerally; results committed to preflight report):
- **PM1**: vertical-root sign flip (Root 2 or 3 in `0x2E74` code) — analog of adapter's H-root mutant, on the V axis. Expected: detected at Phase 1's Δ threshold.
- **PM2**: band lookup index off-by-one — swap adjacent bands. Expected: detected at pixel-column threshold.
- **PM3**: dilation transform inverse-Jacobian error (bump identity Jacobian by 1e-3) — Phase 2 relevant. Expected: detected at Phase 2's dilation-width criterion.

If ANY preflight mutant is NOT detected by the rubric: dame REFORGE with `blocked_reason: rubric_insensitive` + mutant description; operator amends rubric v1.3 to strengthen the criterion before Phase 1 can dispatch.

If ALL preflight mutants detected: dame records BLESS in preflight report; chevalier is cleared to open Phase 1 PR.

## What dame explicitly does NOT judge

- **Perceptual "does this look right"** beyond the rubric's numeric SSIM / Δ / pixel-count / grep-check criteria. Final visual sign-off is operator's `cargo run -p text --features slug` + `cargo run -p atoms-on-wgpu --features slug` + `cargo run -p native-showcase` inspection at mission COMPLETION (per CHARTER § Interaction points Point 2).
- **Whether the choice of variant** in Phase 4 is "the best" — dame verifies the chosen variant's criteria; the choice is chevalier's judgment within the CHARTER's execution_oracle.
- **Whether the code is "clean" or "well-structured"** — dame is not a code reviewer; that's Codex's mid-mission role (chevalier dispatches Codex code-review per CHARTER § Per-phase loop step 4). Dame verifies deliverable outcome per rubric.
- **Whether the chevalier's commit messages are good** — same reason.
- **Any criterion not enumerated above** — if it's not in this rubric, it's not dame's job.

## Ambiguity handling

If dame encounters a criterion where:
- The frozen Codex plan (`codex-8-step-plan.md`) says X
- The adapter's `reference.wgsl` implements Y (X ≠ Y)
- The rubric asks for behavior Z (Z ≠ X and Z ≠ Y)

→ Dame returns REFORGE with:
- `blocked_reason: oracle_ambiguity`
- Structured breakdown: plan says X (line reference), adapter says Y (WGSL line reference), rubric says Z (rubric line reference)
- Suggested operator amendment: pick one authoritative source OR add a new criterion resolving the ambiguity

Chevalier CANNOT advance past this REFORGE without operator amending the rubric (which requires a CHARTER-level operator ratification per CHARTER § Amendment protocol).

If dame is uncertain whether a criterion is met (e.g. per-channel Δ = 4.1/255 vs threshold 4/255):
→ REFORGE (numeric threshold is the decision — dame is not authorized to round in chevalier's favor).

If dame is uncertain due to inability to run a check (e.g. adapter fails to build, docker unavailable):
→ REFORGE with the specific blocked_reason from CHARTER § Blocked signal.

## Dame's decision output format

Dame produces a machine-readable verdict for chevalier to parse:

```yaml
mission: mkui-slug-rewrite
phase: 1                                  # 1, 2, 3, or 4
chevalier_pr: <N>
chevalier_head_sha: <sha>
dame_review_timestamp: <ISO-8601>
oracle_artifacts_read:
  - docs/chevalier/mkui-slug-rewrite/CHARTER.md@<ratification-sha>
  - docs/chevalier/mkui-slug-rewrite/dame-rubric.md@<ratification-sha>
  - docs/chevalier/mkui-slug-rewrite/codex-8-step-plan.md@<ratification-sha>
  - docs/chevalier/mkui-slug-rewrite/reference-harness@9f76af3
adapter_regenerated_at: <ISO-8601>
adapter_sensitivity_recheck: PASS         # known-good rerun byte-identical
storage_mode_used: rgba32float            # or rgba8unorm-16bit if chevalier's implementation uses that
verdict: BLESS                            # or REFORGE
verdict_reason: |
  All 14 Phase 1 BLESS criteria passed. Per-channel Δ max=3/255 across all
  24 comparisons; differing pixels max=6 (on `g` at 2×); SSIM min=0.997.
  fwidth() present at slug.wgsl:157. scale_px_per_unit no longer used as
  AA width. Vertical band upload at lib.rs:189. Root eligibility at
  slug.wgsl:140-165 matches adapter/reference.wgsl:98-105. All existing
  tests pass.
failed_criteria: []                       # populated on REFORGE
provenance:                               # citations for numeric measurements
  per_channel_delta_report: /tmp/dame-phase1-delta.csv
  ssim_report: /tmp/dame-phase1-ssim.csv
```

REFORGE variant fills `failed_criteria: [{id, description, actual, expected, provenance}]` and sets a `blocked_reason` if applicable.

## Amendment protocol

The rubric is immutable within a mission (see CHARTER § Immutability). If chevalier or dame surfaces that a criterion is:
- Impossible to satisfy (mathematically or given available tools)
- Contradicted by newer oracle information
- Ambiguous (per § Ambiguity handling above)
- Insensitive (fails preflight sensitivity test — see § Dame's own preflight sensitivity test)

→ Operator ratifies an amendment to this document, dating the amendment + citing the trigger. Dame checks the git log for amendment commits since the mission-start SHA and uses the current version at check time.

Chevalier CANNOT amend the rubric. If chevalier disagrees with a dame verdict, chevalier posts BLOCKED.md with `blocked_reason: dame_verdict_dispute` — operator arbitrates.

## Version history

- **v1.0-draft** (2026-07-27, mkui orchestrator) — initial draft per handoff at `strategy/goals/mkui-slug-rewrite/HANDOFF-TO-MKUI-2026-07-27.md`. Location: `strategy/goals/mkui-slug-rewrite/`.
- **v1.1-draft** (2026-07-27, mkui orchestrator, same-day revision) — added Phase 0 (chevalier-authored harness). Location: `strategy/goals/mkui-slug-rewrite/`.
- **v1.2** (2026-07-28, mkui orchestrator, post Codex R1 review + adapter merge) — this document at ratification-SHA `b338bd9`. Location: `docs/chevalier/mkui-slug-rewrite/` (mkui repo).
- **v1.2.1** (2026-07-28, mkui orchestrator, amendment 1 per chevalier Phase 2 BLOCKED.md `oracle_ambiguity`) — added byte-identical-to-reference exception clause to § Phase 2 (N) "No thin-gap regressions" criterion. Also fixed stale CHARTER § Blocked signal path (`strategy/goals/...` → `docs/chevalier/...`). Awaits operator ratification via merge of amendment PR.

### v1.2 changes vs v1.1

**Removed:**
- § Phase 0 (chevalier-authored harness) — replaced by pre-ratified adapter at PR #158 (`9f76af3`)
- References to `strategy/goals/mkui-slug-rewrite/reference-harness/` — replaced with `docs/chevalier/mkui-slug-rewrite/reference-harness/`
- "Lengyel 2027" attribution (Codex R1 defect fix — future-dated + unsupported)

**Added:**
- § Threshold calibration — thresholds now calibrated against adapter's actual sensitivity test data (76 pixels, Δ=153/255 signal; 4/255 detection threshold; 8/255 detection floor)
- § Dame's own preflight sensitivity test (per miky #158 N3) — 3 mutants (vertical-root, band lookup, dilation transform) to prove rubric detects beyond adapter's committed mutant
- § Reference golden generation § Docker availability check (per miky #158 N2) — `blocked_reason: docker_unavailable`
- § Reference golden generation § Storage-mode selection (per miky #158 N1) — dame runs both `rgba32float` and `rgba8unorm-16bit` modes, matches chevalier's implementation mode
- § Reference golden generation § Adapter drift check — `blocked_reason: adapter_drift`
- Phase 4 merge-gate note (per Codex R1 P1 #6) — dame BLESS ≠ auto-merge for Phase 4; operator visual smoke is the gate

**Fixed:**
- Phase 3 snap-test math (Codex R1 defect fix) — piecewise-constancy within quantization cells, NOT byte-identical across `[0.0, 0.99]`
- Phase 4 SSIM comparison (Codex R1 defect fix) — each size compared to its OWN reference, NOT cross-size 12px vs 48px
- Phase 4 YOLO scope for `docs/architecture/` + `crates/mkui-text/` (via CHARTER § YOLO scope § Scope directories)
- PNG alpha [0,1] tautology (Codex R1 defect fix) — replaced with pre-clamp float-buffer inspection; if chevalier doesn't expose intermediate, documented as `dame_infrastructure_gap`
- Phase 1 harness glyph contract now uses adapter's ratified glyph set (H, A, V, M, g, o, plus, pipe — 8 total including Codex R1's requested `+` and `|`)
- Phase 2 scope narrowed (Codex R1 defect fix) — bounded half-physical-pixel only, NOT transform-aware dynamic dilation
- SSIM / Δ / pixel-count thresholds calibrated against adapter's sensitivity data (Codex R1 defect fix — was "assertions", now calibrated gates)
- `provider: codex, model: sonnet` invalid pairing removed — dame invocation is via CHARTER § verification_oracle.dame_invocation.command (existing `miky agent assign --task pr-review --provider codex --brief-file` shape)
- Rubric-tampered detection now via cross-phase invariant `git log` check on the mission-input docs (Codex R1 P0 #4 immutability)
- Adapter-tampered detection via cross-phase invariant `git log` check on `reference-harness/` (Codex R1 P0 #1 oracle independence)
