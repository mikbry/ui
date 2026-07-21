# Contributing to mkui

## CI cost discipline

GitHub Actions billing for mkui is real money (~$1 per full PR CI run; a
substrate PR with 3–5 review rounds is 3–5× that). `.github/workflows/ci.yml`
already trims cost automatically:

- **`fail-fast: true`** on the package-aware `test` matrix — a failing feature
  combo stops the matrix instead of letting the other legs burn runner minutes.
- **Path-gated code jobs** — the GPU (Lavapipe), Python-binding, example,
  release, MSRV, and backend-matrix jobs skip on docs-only PRs. The `changes`
  job computes whether any code path (`crates/`, `examples/`, `bindings/`,
  workflows, Cargo manifests, toolchain/deny config) was touched and gates the
  expensive jobs with `needs: changes` + `if:`. A markdown-only PR runs ~20
  jobs instead of the full ~28.

You do not need to do anything for the two mechanisms above — they are automatic.

### `[skip ci]` for iteration commits

During a Codex / reviewer round, agents commonly push 3–5 iterative fix commits
to a PR. **Every push triggers a full CI run.** If only the final
"ready-for-review" commit needs CI, mark the intermediate commits so CI skips
them:

- Include **`[skip ci]`** (or `[ci skip]`) anywhere in the commit message of an
  intermediate, work-in-progress push. GitHub Actions natively skips the
  workflow for that commit — no workflow change is required.
- **Drop the marker on the final push** of the round (the one you actually want
  reviewed / merged) so the full matrix runs against the state that will land.

Example fix-round sequence on a PR branch:

```
git commit -m "fix(core): address review nit — rename field [skip ci]"
git commit -m "fix(core): address review nit — tighten bound [skip ci]"
git commit -m "fix(core): final review pass — ready for CI"   # <- no marker, CI runs
```

**Orchestrator fix-prompt template.** When dispatching a fix agent to iterate on
review feedback, include this instruction verbatim:

> Include `[skip ci]` in the commit message of every intermediate fix commit
> during this review round. Drop it only on your final "ready for review" push
> so CI runs once against the state that will merge. Never `[skip ci]` the final
> push — the full matrix must be green before merge.

Never `[skip ci]` a push you intend to merge from — a merge must always be
gated by a green run of the full matrix against the exact merged state.

### Anti-patterns (do not do these to "save CI")

- **Don't** downgrade macOS runners to `macOS-standard` (2× the 3-core cost).
- **Don't** reduce workspace test coverage — the goal is cheaper CI, not
  less-tested code.
- **Don't** remove the `fmt` / `clippy` / `test-doc` jobs — each is <$0.05/run
  and catches real regressions.
- **Don't** add speculative caching — the current Rust cache is sufficient;
  more caching adds cache-poison risk for little cost win.
