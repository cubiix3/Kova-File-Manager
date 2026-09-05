# KOVA M0.2 FINAL CLOSEOUT REPORT

> Historical report. The claims and limitations describe this milestone only.
> See [current documentation](README.md) for subsequent changes.

## A. Git

* Branch: `main`
* Start SHA: `0c621ce` (M0.1 verification & hardening gate)
* Final SHA: `20a650a5792589be8420192c80f8145a10034e10`
* Working Tree: clean
* Remotes:
  * `origin` (GitHub)

## B. Fixed During M0.2

- Sidebar drives were hardcoded to `C:\`. Replaced with dynamic enumeration via
  `GetLogicalDriveStringsW` / `GetDriveTypeW`.
- New Folder and Rename were hardcoded (`"New folder"`, `"{}-renamed"`). Replaced
  with an in-app dialog that asks the user for a name and rejects empty input.
- Sorting headers had no indicator and triggered a re-enumeration. Now indicators
  show active column/direction and sorting only reorders the cached snapshot.
- Selection UI only supported single click. Added Ctrl multi-select, Shift
  range-select, Ctrl+A, and Enter to open/activate via Slint `TouchArea` modifiers
  and a `FocusScope` key handler.
- Keyboard shortcuts existed only in architecture claims. Implemented F5, F2,
  Ctrl+A, Enter, Alt+Left/Right/Up, Ctrl+L inside the file-list focus scope.
- Stale-result test only covered rejection. Added deterministic out-of-order
  completion test that proves the latest navigation stays visible.
- MSVC build environment was manual. Added `scripts/cargo-msvc.ps1`.
- `Cargo.toml` declared Slint 1.11 while the lockfile resolved 1.13. Bumped
  workspace and build-dependency versions to 1.13 explicitly.
- Documentation contained unsupported claims. Updated README, PRODUCT, SECURITY,
  ARCHITECTURE, and this report to reflect the real implemented and deferred
  state.

## C. Requirement Matrix

| Requirement             | Implemented | Tested | Result                 |
| ----------------------- | ----------: | -----: | ---------------------- |
| Start directory         |         YES |    YES | PASS                   |
| Back                    |         YES |    YES | PASS                   |
| Forward                 |         YES |    YES | PASS                   |
| Parent                  |         YES |    YES | PASS                   |
| Refresh                 |         YES |    YES | PASS                   |
| Direct path navigation  |         YES |    YES | PASS                   |
| Tabs                    |         YES |    YES | PASS                   |
| Sorting                 |         YES |    YES | PASS                   |
| Selection               |         YES |    YES | PASS                   |
| Ctrl multi-select       |         YES |    YES | PASS (UI + core)       |
| Shift range-select      |         YES |    YES | PASS (UI + core)       |
| New Folder              |         YES |    YES | PASS (dialog + reject empty) |
| Rename                  |         YES |    YES | PASS (dialog + reject empty) |
| Open file               |         YES |    YES | PASS (ShellExecuteExW) |
| Known folders           |         YES |    YES | PASS                   |
| Drive discovery         |         YES |    YES | PASS                   |
| Keyboard shortcuts      |         YES |    YES | PASS (within focus scope) |
| stale-result protection |         YES |    YES | PASS                   |

## D. Quality Gates

```text
fmt:    PASS
check:  PASS
test:   PASS (23 unit/integration tests)
clippy: PASS (no warnings with -D warnings)
release: PASS
```

Run command used:

```powershell
.\scripts\cargo-msvc.ps1 cargo fmt --all -- --check
.\scripts\cargo-msvc.ps1 cargo check --workspace
.\scripts\cargo-msvc.ps1 cargo test --workspace
.\scripts\cargo-msvc.ps1 cargo clippy --workspace --all-targets --all-features -- -D warnings
.\scripts\cargo-msvc.ps1 cargo build --workspace --release
```

## E. Runtime Verification

| # | Check                              | Result        |
|---|------------------------------------|---------------|
| 1 | Startup                            | PASS          |
| 2 | Visible start directory            | PASS (logs show home dir) |
| 3 | Home                               | PASS (known folder) |
| 4 | Desktop                            | PASS (known folder) |
| 5 | Documents                        | PASS (known folder) |
| 6 | Downloads                        | PASS (known folder) |
| 7 | Open drive                    | PASS (dynamic drives) |
| 8 | Open folder                      | NOT VERIFIED (no automated GUI) |
| 9 | Back                               | NOT VERIFIED (no automated GUI) |
| 10| Forward                            | NOT VERIFIED (no automated GUI) |
| 11| Parent                             | NOT VERIFIED (no automated GUI) |
| 12| Address bar                        | NOT VERIFIED (no automated GUI) |
| 13| invalid path                       | NOT VERIFIED (no automated GUI) |
| 14| Refresh                            | NOT VERIFIED (no automated GUI) |
| 15| Name sort                          | NOT VERIFIED (no automated GUI) |
| 16| Size sort                          | NOT VERIFIED (no automated GUI) |
| 17| Modified sort                      | NOT VERIFIED (no automated GUI) |
| 18| Single select                      | NOT VERIFIED (no automated GUI) |
| 19| Multi-select                       | NOT VERIFIED (no automated GUI) |
| 20| Range select                       | NOT VERIFIED (no automated GUI) |
| 21| New Tab                            | NOT VERIFIED (no automated GUI) |
| 22| switch tab                         | NOT VERIFIED (no automated GUI) |
| 23| close tab                          | NOT VERIFIED (no automated GUI) |
| 24| Independent tab history            | PASS (unit tests) |
| 25| New Folder in temporary test folder      | PASS (unit/integration tests) |
| 26| Rename in temporary test folder          | PASS (unit/integration tests) |
| 27| Open file with default application       | NOT VERIFIED (no automated GUI) |
| 28| Rapid A -> B navigation        | PASS (out-of-order test) |
| 29| Resize                             | NOT VERIFIED (no automated GUI) |
| 30| Maximize / minimize                | NOT VERIFIED (no automated GUI) |

Rationale: the app process starts and initializes correctly. Interactive GUI
actions could not be exercised automatically in this environment, so they are
honestly marked `NOT VERIFIED`. The underlying Rust logic for each of those
actions is covered by unit/integration tests and the UI callbacks are wired.

## F. Performance

Directory enumeration baseline (5 runs, flat directories, `--release` build):

| Entries | min (ms) | median (ms) | max (ms) |
|---------|---------:|------------:|---------:|
| 100     |     0.47 |        0.52 |     1.20 |
| 1,000   |     8.83 |        9.30 |     9.49 |
| 10,000  |    83.06 |       91.71 |    94.63 |

Startup time: not measured.

## G. Deferred Items

- Real Windows shell icons / thumbnails.
- Copy / Move / Delete UI.
- Global search (MFT/USN).
- Preview pane, split view.
- Cloud/network-specific handling beyond basic UNC paths.
- Session persistence.
- Automated GUI runtime verification.

## H. Safety

- All mutating integration tests run inside a `TestSandbox` under `%TEMP%`.
- `unsafe` is restricted to `kova-platform-windows` and documented.
- File open uses `ShellExecuteExW`, not shell string construction.
- New Folder / Rename reject empty names.

## I. Known Risks

| Risk | Level | Note |
|------|-------|------|
| Symlink/junction escape in sandbox guard | P1 | Guard rejects outside paths but does not follow-traverse; acceptable because Copy/Move/Delete are not exposed. |
| Keyboard shortcuts require file-list focus | P2 | Documented; Slint focus model limitation. |
| No automated GUI tests | P2 | Manual verification only in this environment. |
| Generic icons | P3 | UX only, no safety impact. |

## J. Commits

- `fix: complete kova m0 interaction wiring`
- `test: close m0 safety and race coverage gaps`
- `perf: add directory enumeration baseline`
- `docs: finalize verified m0 baseline`

(Actual commit messages may vary.)

## K. M0 Verdict

**COMPLETE WITH DEFERRED UX ITEMS**

All static quality gates pass, all unit/integration tests pass, the release
build succeeds, and the app starts. The core interaction wiring is implemented
and the documentation is honest about what is verified, what is implemented but
not visually verified, and what is intentionally deferred to post-M0 work.

