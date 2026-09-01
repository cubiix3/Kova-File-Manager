# Kova Security and Data Safety — M0

## Principle

Kova is a file manager. Broken animations are bugs; broken file operations are
unacceptable. No untested low-level code may destructively modify real user
files.

## Unsafe Policy

`unsafe` is allowed only in `kova-platform-windows` and only where required by
the Windows API. Every `unsafe` block contains a `SAFETY:` comment explaining
why the preconditions hold.

Current `unsafe` usage:

1. `SHGetKnownFolderPath` in `known_folders.rs`
   - CoInitializeEx is called first.
   - The returned PWSTR is freed with `CoTaskMemFree`.
   - A helper converts the wide string to an `OsString` before freeing.

2. `ShellExecuteExW` in `file_ops.rs`
   - COM is initialized in apartment-threaded mode.
   - Operation and file wide strings are valid for the call duration.
   - Null HWND and parameters are explicit.

3. `GetLogicalDriveStringsW` / `GetDriveTypeW` in `volumes.rs`
   - Pre-allocated buffers sized for 26 drive letters.
   - Wide strings are null-terminated before passing.

## Destructive Operations

For M0 only `New Folder` and `Rename` are exposed in the UI, and they operate on
the current directory. Integration tests use `TestSandbox` which enforces that
mutating targets live under a unique temporary root.

Copy / Move / Delete are **not** exposed in the product UI. They are prepared
as core functions but will only be enabled once they use Windows
`IFileOperation` or equivalent safe APIs and are fully tested in the sandbox.

## Path Handling

- Paths use `Path` / `PathBuf` internally.
- User input is normalized (forward slashes to backslashes) but never blindly
  resolved across junctions.
- Long paths and UNC paths are preserved; no length assumptions are made.

## Logging

- `tracing` is used with an environment filter.
- File contents are never logged.
- Full paths may appear in debug-level diagnostics only.

## Test Safety

All mutating integration tests:

1. Create a unique directory under `%TEMP%\kova-tests\`.
2. Verify the target path is inside that root before mutating.
3. Clean up after the test.
