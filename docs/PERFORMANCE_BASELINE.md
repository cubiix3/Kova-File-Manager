# Kova Performance Baseline — M0

All numbers below are real measurements from the current Windows machine. No
estimates or marketing numbers.

## Environment

- OS: Windows 11 24H2 (Build 26200.9278)
- CPU/ RAM: (host machine)
- Disk: local NVMe / SSD
- Rust: rustc 1.95.0 stable-x86_64-pc-windows-msvc
- Build: `--release`

## Startup Time

Not measured in this baseline. The app launches and shows a window; a precise
"first paint" measurement is deferred to a later milestone when Slint exposes
a reliable paint event or we add an internal timer span.

## Directory Enumeration

Measured with `crates/kova-ops/src/enumerate.rs::enumerate_directory` on
synthetic flat directories under `%TEMP%\kova-perf\`. Five runs per size;
reported values in milliseconds.

| Entries | min (ms) | median (ms) | max (ms) |
|---------|---------:|------------:|---------:|
| 100     |     0.47 |        0.52 |     1.20 |
| 1,000   |     8.83 |        9.30 |     9.49 |
| 10,000  |    83.06 |       91.71 |    94.63 |

Command used:

```powershell
.\scripts\cargo-msvc.ps1 cargo test --workspace -- --ignored enumerate_directory_performance_baseline --nocapture
```

## UI

- File list uses Slint `ScrollView` over a model of already-loaded entries.
- No per-frame icon decoding.
- Sorting and selection run in core on the main thread but operate only on the
  already-loaded snapshot; they do not touch disk.

## Notes

- These are baseline numbers, not targets.
- Heavy optimization (MFT/USN, icon cache, parallel enumeration) is planned
  for later milestones after the core is trustworthy.
