# Live updates and resource verification

Verified on Windows on 6 September 2026, after the NextGen interface update.

## Changes

- Native recursive directory notifications reconcile external file creation,
  writes, renames and deletion in the background. A 150 ms quiet period coalesces
  bursts; a 600 ms maximum debounce prevents continuous writes from starving
  updates. These are scheduling targets, not filesystem latency guarantees.
- Background refresh preserves selection/search and stays out of inline rename
  editing. Inactive tabs and the parents of collection references are monitored.
  Notification handles are owned by one worker and released when roots change.
- The Shell icon cache is capped at 1,024 entries, including failed lookups.
  UI image slots are reclaimed and reused when no live tab references them;
  filtered-out entries retain valid slots. Request/result channels are bounded
  at 256/64; obsolete work is discarded, and queue saturation never blocks the UI.
- Closing a tab cancels enumeration and removes its generation counter. Home
  cancels any preceding directory enumeration. Worker shutdown aborts remaining
  tracked enumeration tasks; an already running OS call may finish later.
- Library saves compare their loaded version under an exclusive cross-process
  file lock. A stale window cannot silently replace newer pins, tags or collections.
  Unchanged windows do not write their old snapshot on exit.
- The test sandbox fails closed on unresolved paths and returns validated absolute
  paths. The ignored performance fixture now owns a unique temporary directory.

## Verification

`cargo fmt --all -- --check`, workspace/all-targets check, workspace tests,
Clippy with all targets/features and `-D warnings`, and the release build passed.
There are **79 passing tests**, with four explicitly ignored interactive/performance
tests. Regression coverage includes real directory notifications, 10,000 closed-tab
generations, cache eviction, 4,000 UI icon replacements, bounded delivery, filtered
selection preservation and refusal to overwrite another window's library save.

The unique-directory enumeration performance test was also run explicitly:
five debug-build runs gave medians of 4.59 ms for 1,000 entries and 94.86 ms for
10,000 entries. These measure enumeration on the test volume, not end-to-end
rendering or network latency.

In the release application, an externally written EXE changed from 8 KiB to
16 KiB in both the list and inspector without F5; the selected path remained
selected. An external write during F2 editing preserved the in-progress name.
Escape cancelled the edit. Native Shell menus were opened in the final build.

A controlled navigation test used 20 temporary folders with 80 distinct EXE
paths each: 60 folder visits, plus six tab open/close cycles. These files exercised
Shell icon lookup; they were not executed.

| Completed visits | Private memory (MiB) | Handles | GDI objects | USER objects |
| --- | ---: | ---: | ---: | ---: |
| 10 | 27.01 | 527 | 56 | 24 |
| 20 | 28.82 | 526 | 56 | 24 |
| 30 | 28.81 | 528 | 56 | 24 |
| 40 | 28.58 | 528 | 56 | 24 |
| 50 | 28.57 | 526 | 56 | 24 |
| 60 | 29.28 | 524 | 56 | 24 |

This demonstrates stable resource use in the tested workload after warm-up, not
proof that every installed codec or Shell extension is leak-free.

## Security review boundaries

A static review covered repository-owned Rust, Slint, native ownership/clipboard
boundaries, mutation paths, integration scripts, installer and workflows. It did
not establish an exploitable repository-owned vulnerability. The reliability and
test-safety observations above were fixed after that source snapshot was reviewed.

Third-party decoder/extension internals and dependency advisories were outside
the scan. Native provider calls are synchronous and do not have a hard process-wide
memory or timeout guarantee. Explicit network paths still depend on Windows and
the provider. Home drive discovery still refreshes with F5.

See [product images](DEMO.md) for fresh captures of the resulting release build.
