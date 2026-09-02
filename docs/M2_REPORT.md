# KOVA M2 REPORT — NATIVE SHELL + FILE OPERATIONS + UI POLISH

Datum: 2026-09-02
Branch: `main`
Basis: M1-Final (`b12485c`)

---

## A. Git

```text
cec0b7d  feat(platform): native shell context menu, IFileOperation ops thread, Explorer file clipboard, drive capacity
377b553  feat(desktop): wire native shell menus and clipboard ops, product-quality UI polish
<this commit>  docs: add m2 report
```

---

## B. Umsetzung

### B.1 Natives Windows Shell-Kontextmenü (Dateien/Ordner)

* `kova-platform-windows::shell_menu`: echtes Explorer-Menü via `IContextMenu`
  (Desktop-Shellfolder + `GetUIObjectOf` mit **allen** selektierten Pidls →
  Mehrfachauswahl wie im Explorer).
* `IContextMenu2`/`IContextMenu3` Message-Forwarding (`WM_INITMENUPOPUP`,
  `WM_DRAWITEM`, `WM_MEASUREITEM`, `WM_MENUCHAR`) über ein eigenes verstecktes
  Host-Fenster (`KovaShellMenuHost`) — **kein** Subclassing des Slint-Fensters,
  keine Hooks, kein `unsafe impl Send`. Shell-Extension-Einträge (7-Zip, Git,
  „Öffnen mit“, Eigenschaften) rendern und funktionieren dadurch korrekt.
* Verb-Invocation über `CMINVOKECOMMANDINFOEX` mit MAKEINTRESOURCE-Offset
  (`CMD_FIRST..=0x7FFF`), Out-of-Range-Ids werden als Raw-Id invoke-t,
  `CMIC_MASK_PTINVOKE` + Cursor-Position für Extensions.
* Rechtsklick-Verhalten wie Explorer: klickte Zeile in der Auswahl → Menü für
  die ganze Auswahl, sonst nur für diese Zeile. Nach invoke → Refresh.
* Leere Fläche: weiter Kova-Menü (New Folder, **Paste**, Refresh) via Slint
  `ContextMenuArea` — nicht überlagert.
* COM STA auf dem UI-Thread beim Start sichergestellt (`ensure_com_sta`).

### B.2 Copy / Cut / Paste / Move / Delete

* **Clipboard** (`clipboard.rs`): `CF_HDROP` (eigener DROPFILES-Encoder/Parser,
  unit-getestet) + „Preferred DropEffect“ (COPY=1 / MOVE=2) → Explorer-
  kompatibel in beide Richtungen.
* **IFileOperation** auf dediziertem Thread (`shell_ops.rs`, eigener COM-STA,
  `catch_unwind` → Fehler werden als Outcome gemeldet statt den Thread zu
  töten): CopyItems / MoveItems / DeleteItems mit `FOF_ALLOWUNDO` (Papierkorb)
  + native Progress-/Konflikt-Dialoge. UI-Thread blockiert nie.
* Tastatur: Ctrl+C / Ctrl+X / Ctrl+V / Entf, Multi-Selection via Strg/Shift.
* User-Cancel (0x800704C7 / 0x800703E3 / COPYENGINE_E_USER_CANCELLED) wird als
  „abgebrochen“-Status behandelt, echte Fehler als Status + Fehlerdialog.
* Nach jedem Ops-Abschluss wird die Ansicht automatisch aktualisiert.

### B.3 Produkt-Qualität UI

* Toolbar mit echten Vektor-Icons (Path-Glyphen: Back/Forward/Up/Refresh/
  New Folder), flache Toolbars, Trennlinien, fokussierte Addressbar mit
  Accent-Border.
* Tabs: Pill-Style, aktive Tabs mit Accent-Underline, Close-Button nur bei
  Hover/Aktiv, dichteres Layout.
* Sidebar: „Quick Access“ + „Drives“-Sektionen, dichtere Zeilen, **Laufwerke
  mit Usage-Bar** (GetDiskFreeSpaceExW) + „X GB free of Y GB“-Detail, Danger-
  Farbe >90 % Belegung.
* Dateiliste: 26-px-Zeilen, sanftere Hover-/Selection-States, konsistente
  Spaltenbreiten über `Metrics`, Header-Alignment stimmt mit Zeilen überein.
* Statusbar reduziert auf Status + „N items · M selected“.
* Dialog: abgerundet, Accent-OK.

### B.4 UX-Fixes (Runtime gefunden)

* **Selection überstand Navigationen** (alte Indizes leuchteten in der neuen
  Ansicht) → Selection wird bei Navigate/Back/Forward geleert (Explorer-
  Verhalten).
* **Ctrl+L** fokussiert jetzt die Adressleiste mit Select-All (Standard-
  Shortcut, vorher sinnlos „re-navigieren“).
* Nach Enter in der Adressleiste versucht Kova, den Fokus zurück auf die
  Liste zu geben.

---

## C. Quality Gates

```text
cargo fmt --all -- --check:            PASS
cargo check --workspace --all-targets: PASS
cargo test --workspace:                PASS (38 passed, 1 ignored: perf baseline)
cargo clippy -D warnings:              PASS
cargo build --release:                 PASS (~30 s)
```

Neue Tests: HDROP-Buffer-Roundtrip + ANSI/Short-Image-Ablehnung,
Clipboard-Files-Roundtrip (echte Zwischenablage, gegen Race serialisiert —
der parallele Zugriff zweier Clipboard-Tests hatte STATUS_HEAP_CORRUPTION
erzeugt, jetzt via Mutex serialisiert), Selected-Paths-Reihenfolge,
Drive-Capacity, Shell-Op-Labeling.

---

## D. Runtime-Verifikation (real, Release-Binary, UIA + SendInput)

Sandbox: `%TEMP%\kova-m2-run` (alpha.txt, beta.log, SubFolder) + Staging-
Ordner für Paste-Quelle. Navigation log-verifiziert, Dateisystem als Ground
Truth.

| Test | Ergebnis |
| --- | --- |
| Navigation in Sandbox (Ctrl+L, Adressbar) | PASS (log) |
| Ctrl+C → CF_HDROP alpha.txt | PASS (PowerShell liest `GetFileDropList`) |
| Multi-Selection (Ctrl+Click) | PASS (Status „3 items · 2 selected“ via UIA) |
| Ctrl+X → DropEffect=MOVE(2) | PASS |
| Ctrl+V (cut) → beta.log nach SubFolder **verschoben** | PASS (Disk + Log `entries=2`) |
| Explorer-CF_HDROP → Ctrl+V **kopiert** paste_me.txt | PASS (Disk + Log `entries=3`) |
| Entf → paste_me.txt **Papierkorb** | PASS (Disk + Log `entries=2`) |
| Rechtsklick → **natives Shell-Menü** (#32768-Popup) | PASS |
| Screenshots | `.rivet_temp/m2_view.png`, `.rivet_temp/m2_shell_menu.png` (PrintWindow, echter dunkler Inhalt verifiziert) |

Hinweise:

* 7-Zip und Git sind auf dieser Maschine installiert; das Menü wird aus dem
  echten `IContextMenu` aufgebaut, damit erscheinen deren Einträge nativ.
  Eine item-level UIA-Verifikation des Popup-Menüs war nicht möglich (Win32-
  Menüs exponieren keine MenuItem-Elemente über den Popup-HWND) — bitte
  visuell bestätigen (Screenshot liegt vor).
* Regressionen M1 (Icons, Tabs, Back/Forward, Sidebar, Addressbar, New Folder,
  Rename, Auswahl/Multi-Selection, Doppelklick, Resize) unverändert in der
  Pipeline; Row-Kontextmenü-Einträge „Open/Rename/Copy Path“ wanderten
  bewusst ins native Shell-Menü bzw. Keyboard (F2, Entf, Ctrl+C/X/V/T).

---

## E. Bekannte Grenzen / Deferred

* „This PC“-Übersicht als eigene Ansicht: geprüft, bewusst vertagt (Sidebar-
  Laufwerke mit Usage-Bars decken den Anwendungsfall ab); M3-Kandidat.
* Drag & Drop, Undo, Multi-Select-Rename: M3+.
* Zwischenablage-Konflikt-Dialoge kommen nativ von IFileOperation (kein
  eigenes UI nötig).
* Die Tests mutieren die Zwischenablage nur, wenn keine Dateien dort lagen
  (guarded), sonst nur Lese-Verifikation.