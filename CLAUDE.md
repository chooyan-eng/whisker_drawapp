# whisker_drawapp — agent guide

A [Whisker](https://whisker.rs) app: a Rust-first cross-platform mobile UI
framework that renders native widgets via the Lynx engine. Single-crate
workspace; the app entry point is `src/lib.rs` (`#[whisker::main]`).

## Reference docs (read these before using unfamiliar APIs)

When running inside the Whisker AI container, the **framework source and
docs are mounted read-only** at `$WHISKER_DOCS` (`/workspace/whisker-src`):

- `/workspace/whisker-src/docs/` — architecture, reactivity, Lynx integration, etc.
- `/workspace/whisker-src/crates/` — the framework crates (grep for real APIs/signatures).
- `/workspace/whisker-src/examples/` — working example apps.

Online docs: https://whisker.rs/docs (Elements, Events, Reactivity, Control
Flow, First-party Modules). Prefer the mounted source as the source of truth.

## Building & checking

- **iOS is NOT available here** — it needs Xcode/macOS and runs only on the
  Mac host (`whisker run ios`). Do visual UI verification there.
- In-container, compile-check against the Android target:
  ```bash
  cargo check --target aarch64-linux-android
  ```
- Format (also formats `render!` / `css!` macro bodies):
  ```bash
  whisker fmt
  ```
- `whisker doctor --no-ios` reports the Android toolchain status.

## Conventions

- UI is built with the `render!` macro; styling via inline CSS strings or the
  `css!` macro. Reactivity is fine-grained signals (`RwSignal`, `computed`).
- Dynamic lists use `ForEach(each:, key:, children:)`; conditionals use `Show`.
- First-party modules (e.g. `whisker-svg`, `whisker-image`) are versioned in
  lockstep with the core `whisker` crate — use a matching major.minor (`0.4`).
- There is no text-input element yet in Whisker.
