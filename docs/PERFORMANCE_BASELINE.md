# Kova Performance Baseline — M0

All numbers below are real measurements from the current Windows machine. No
estimates or marketing numbers.

## Environment

- OS: Windows 11 24H2 (Build 26200.9278)
- CPU/ RAM: (to be filled after measurement)
- Disk: local NVMe / SSD
- Rust: rustc 1.95.0 stable-x86_64-pc-windows-msvc
- Build: `--release`

## Startup Time

| Run | Window Visible (ms) |
|-----|---------------------|
| 1   | TBD                 |
| 2   | TBD                 |
| 3   | TBD                 |

Measured with `tracing` span around `MainWindow::new()` through first paint
event.

## Directory Enumeration

| Entries | Request → Result (ms) |
|---------|------------------------|
| 100     | TBD                    |
| 1,000   | TBD                    |
| 10,000  | TBD                    |

Tests use synthetic directories created under `%TEMP%\kova-perf\`. Reported
after M0 runtime verification.

## UI

- File list uses Slint `ListView`, which virtualizes items.
- No per-frame icon decoding.
- Sorting and selection run in core on the main thread but operate only on the
  already-loaded snapshot; they do not touch disk.

## Notes

- These are baseline numbers, not targets.
- Heavy optimization (MFT/USN, icon cache, parallel enumeration) is planned
  for later milestones after the core is trustworthy.
