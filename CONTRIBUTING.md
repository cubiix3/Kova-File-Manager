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
  .\scripts\cargo-msvc.ps1 cargo test --workspace
  ```

## Required quality gates

CI runs these on every push and pull request; please make sure they pass
locally before opening a PR:

```powershell
cargo fmt --all -- --check
cargo check --workspace --all-targets
cargo test --workspace
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

## Architecture rules

- Never block the UI thread with filesystem or shell work — route it
  through the worker/event-pump architecture (see
  [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md)).
- `kova-core` stays platform-independent: no `unsafe`, no Win32, no Slint.
- Every `unsafe` block needs a `SAFETY:` comment.
- UI-visible failures must be surfaced in the status line or dialogs —
  no silently swallowed errors.

## Commits & pull requests

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
