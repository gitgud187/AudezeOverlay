# Contributing

Repository agent instructions and coding boundaries are in [AGENTS.md](AGENTS.md).

Use current stable Rust on Windows. MSVC is the CI distribution target;
`build.ps1` also supports the project's optional local LLVM-MinGW toolchain.
Keep `Cargo.lock` committed. Do not commit `.tools`, `target`, `dist`, captures,
USB traces, settings, credentials or personal font files; see `.gitignore`.

Before a change is ready:

```powershell
cargo fmt --all --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked --all-targets --all-features
cargo run --release --bin preview-layouts
cargo build --release --bin audeze-overlay
```

For window/input changes, quit the running overlay, run
`target\release\audeze-overlay.exe --smoke-test`, and inspect every boolean in
`diagnostics/smoke-v3.json`. Restart your normal overlay afterward. Smoke checks
use sample data. Do not run hardware-control diagnostics without deliberately
opting into their documented effects.

Use conventional commit messages, for example `fix(layout): preserve card size
when dragging` or `feat(ui): animate directional selection changes`. Do not add
co-author trailers. Keep descriptions focused on behavior and relevant checks.

Start with [the architecture guide](docs/ARCHITECTURE.md). Add regression tests
for meaningful state/layout bugs; avoid tests that merely duplicate rendering
implementation details. Inspect rendered previews when changing visuals.
