# Changelog

## 0.2.0 — Windows preview, 2026-09-10

- Restore tabs, active location, search scope and filters, window state, columns,
  gallery and inspector. Save preferences after a short debounce with atomic replacement.
- Use computed folder sizes consistently for display, filtering and sorting.
- Run large-directory filtering and natural sorting off the UI thread; reuse ordering
  and materialize only accessed rows. Reject stale search and navigation results.
- Add accessible Type / Size / Date filters and cancellable recursive folder search.
- Add native Windows drag & drop between folders, tabs, sidebar and Explorer, with
  Copy/Move feedback before dropping and explicit conflict decisions.
- Review and undo supported renames and same-volume file moves with identity and
  destination checks. Copy, delete, replacement and cross-volume moves remain outside Undo.
- Compare incoming/existing names, paths, sizes and dates in conflicts. Keep Both,
  Skip, Replace and apply-to-remaining choices preserve explicit overwrite consent.
- Match the file context menu to the app with icons, grouped actions and shortcuts;
  retain native Shell extensions through More Windows options and Shift+F10.
- Give Home recent folders and favorites; simplify toolbar actions; improve inspector
  path wrapping/copying, tooltips, keyboard shortcuts and accessibility labels.
- Detect drive arrival/removal automatically; show unavailable locations with Retry.
  Use Windows locale formatting and a long-path-aware, per-monitor-DPI manifest.
- Extract search, lazy file model, sidebar, UI synchronization, callbacks, inspector,
  controls and operations modules without replacing the native Shell operation layer.
- Fix Visual Studio discovery, including standalone Build Tools. Add real filesystem,
  long-path and UI interaction coverage plus reproducible 1k/10k/100k benchmarks.

Validation, measured performance and explicit limits: [daily-driver report](docs/DAILY_DRIVER_VERIFICATION.md).

## 0.1.0 — initial Windows preview

First Setup/portable distribution with tabbed native Windows file browsing,
Shell context menus, drive overview and image/text/PDF previews.
