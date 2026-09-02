# Security Policy

## Supported versions

Kova is pre-1.0 and under active development. Only the current `main`
branch receives security fixes; there are no versioned releases yet.

## Reporting a vulnerability

Please use GitHub's **private vulnerability reporting**
(Security tab → "Report a vulnerability"). Do **not** open a public issue
for security reports.

Please include:

- the affected module or code path
- your Windows version and build configuration
- reproduction steps or a minimal example
- your assessment of the impact — especially for anything that could
  cause unintended data loss or modification

## Scope

Kova performs real filesystem operations (copy, move, delete with
Recycle-Bin fallback) driven by user actions. Reports are especially
welcome for:

- unintended data loss or modification in file operations
- path handling issues (e.g. traversal or escaping the intended
  operation scope)
- command invocation through the native shell context menu
- clipboard file-list handling (`CF_HDROP` / DropEffect)
- privilege or sandbox escapes in the test sandbox code

Out of scope: Kova performs no network I/O, no telemetry and no
auto-updates. Vulnerabilities in the underlying `slint` or `windows`
crates should be reported upstream; we track their advisories.

## Data safety principles

The project's data-safety rules for file operations are documented in
[`docs/SECURITY_AND_DATA_SAFETY.md`](docs/SECURITY_AND_DATA_SAFETY.md):
operation tests run in isolated sandboxes, deletes go to the Recycle Bin,
and destructive operations surface native confirmation/conflict dialogs
via `IFileOperation`.