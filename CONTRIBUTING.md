# Contributing to Kova

Thanks for your interest in improving Kova!

## Environment

- Windows 10/11 x64 with the MSVC toolchain (Kova uses real Win32/Shell/COM
  APIs and does not build on other platforms yet).
- Rust stable (selected via `rust-toolchain.toml`) and Visual Studio
  Build Tools with the C++ workload.
- Use `scripts/cargo-msvc.ps1` to run cargo with the Visual Studio
  environment, e.g.:

  ```powershell
  .\scripts\cargo-msvc.ps1 test --locked --workspace
  ```

## Required quality gates

CI runs these on pull requests and pushes to `main`; please make sure they pass
locally before opening a PR:

```powershell
cargo fmt --all -- --check
cargo check --workspace --all-targets
cargo test --workspace
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

## Interaction and performance verification

For changes to navigation, selection, file operations or session restoration,
exercise the actual Windows UI on an interactive desktop:

```powershell
.\scripts\cargo-msvc.ps1 build --locked --workspace --release
.\scripts\test-ui.ps1 -Executable target/release/kova-desktop.exe
```

The test uses isolated files and preferences, checks real file contents, and
retains logs and screenshots under `target/runtime`. It sends keyboard/mouse input
and uses the Windows clipboard, so leave its test window available while it runs.

For changes to enumeration, filtering, sorting or row rendering, use
`scripts/benchmark-ui.ps1 -PrepareFixtures`. It creates 1k/10k/100k test folders;
see the [measurement method and limitations](docs/DAILY_DRIVER_VERIFICATION.md).

Update affected user documentation and the changelog with behavior changes. If a
visible flow changes, refresh its [real product captures](docs/DEMO.md); keep older
milestone evidence labeled as historical. See [Windows packaging](docs/WINDOWS_RELEASE.md)
for installer and packaged-UI verification before publication.

## Architecture rules

- Never block the UI thread with filesystem or shell work — route it
  through the worker/event-pump architecture (see
  [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md)).
- `kova-core` stays platform-independent: no `unsafe`, no Win32, no Slint.
- Every `unsafe` block needs a `SAFETY:` comment.
- UI-visible failures must be surfaced in the status line or dialogs —
  no silently swallowed errors.

## Commits & pull requests

Dependabot checks the Cargo workspace and GitHub Actions weekly on Monday at
09:00 Europe/Berlin. Slint runtime/build updates are grouped together; Windows
bindings have a separate group because even 0.x minor versions can change APIs.
Other minor/patch updates are grouped by ecosystem; remaining major updates get
individual PRs. Dependency PRs require review and passing CI, not automatic merging.
Security updates are enabled separately in the repository settings and are not
restricted to this weekly version-update schedule. Hardcoded tool download URLs
in scripts, such as cargo-about, still need manual maintenance.

- Keep commits logical and small; use conventional prefixes
  (`feat`, `fix`, `docs`, `chore`, `refactor`).
- Describe user-visible behavior changes in the pull request; for bug
  fixes include reproduction steps.
- Bug reports: include your Windows version and steps to reproduce.
  Feature proposals: describe the problem first, then the proposed
  behavior.

## License

By contributing, you agree that your contributions are dual licensed under
[MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at the option of the
maintainers, consistent with the repository license.

## Security

For security-sensitive reports (file operations, path handling, shell menu
invocation, clipboard), please use private vulnerability reporting — see
[`SECURITY.md`](SECURITY.md).
