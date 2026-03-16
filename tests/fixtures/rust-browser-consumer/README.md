# Rust Browser Consumer Fixture

Fixture for bead `asupersync-4l9iw.2`.

Purpose:
- prove the repository-maintained Rust-authored browser lane with a real wasm package layout
- keep the example honest about scope: this is a maintained in-repo workflow, not broad public `RuntimeBuilder` parity for external Rust consumers
- demonstrate structured-concurrency lifecycle behavior through the existing dispatcher/provider helpers

This fixture is executed through:
- `scripts/validate_rust_browser_consumer.sh`

The validation script:
- builds the nested Rust crate with `rch exec -- wasm-pack build ...`
- stages the generated `pkg/` output next to the frontend consumer
- runs a Vite bundle check against the resulting browser artifact

## Layout

- `crate/Cargo.toml`
  Rust-authored wasm package that depends on the root `asupersync` crate under a canonical browser profile
- `crate/src/lib.rs`
  exports a small browser-facing demo that mounts a provider, creates a child scope, completes one task, leaves one task for unmount-driven cancellation, and returns a structured summary
- `src/main.ts`
  initializes the generated wasm package and renders the demo summary into the page
- `scripts/check-bundle.mjs`
  asserts the built Vite output retains both JavaScript and wasm assets

## Boundary Rules

- This fixture is a repository-maintained example for the current Rust-authored browser contract.
- It does not claim a general external Rust-browser bootstrap API beyond what `docs/WASM.md` currently marks as truthful scope.
- It uses the existing wasm dispatcher/provider helpers instead of inventing a new public browser `RuntimeBuilder` story.

## Deterministic Validation

Run the maintained example through the canonical validation path:

```bash
PATH=/usr/bin:$PATH bash scripts/validate_rust_browser_consumer.sh
```

Artifacts are emitted under:

```text
target/e2e-results/rust_browser_consumer/
```
