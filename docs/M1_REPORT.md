# KOVA M1 FINAL REPORT — WINDOWS INTERACTION, ICONS & PRODUCT-QUALITY UI

Datum: 2026-09-02
Branch: `main`
Basis: M0.2-Closeout (`face92d` → `1ebdabb`)

---

## A. Git

* Branch: `main`
* Start SHA: `1ebdabb` (feat: add windows shell icon resolver and clipboard text access)
* Feature SHA: `b12485c` (feat: m1 shell icons, context menus, mouse navigation, ui polish)
* Working Tree: clean nach Abschluss
* Geänderte Dateien: `apps/kova-desktop/{src/main.rs, src/app_state.rs, src/bridges.rs, ui/main.slint}`, `crates/kova-platform-windows/src/shell_icons.rs`

Commits:

```text
b12485c  feat: m1 shell icons, context menus, mouse navigation, ui polish
<this commit>  docs: add m1 report
```

---

## B. Was in M1 umgesetzt wurde

### B.1 Maus Navigation (Back/Forward, XBUTTON1/XBUTTON2)

* Neue Slint-Komponente `NavTouchArea` (erweitert `TouchArea`): reagiert in
  `pointer-event` auf `PointerEventButton.back` / `PointerEventButton.forward`
  und löst exakt dieselben Dispatch-Pfade aus wie die Toolbar-Buttons
  (`AppState.request_back()` / `request_forward()`). Der Dialog-Zustand wird
  respektiert (keine Navigation bei offenem Dialog).
* Eingebaut in: Toolbar, Tabs, Sidebar, Dateiliste (inkl. Zeilen), Statusbar.
  Damit sind die Maustasten XBUTTON1 (Back) und XBUTTON2 (Forward) über die
  **bestehende Slint-Input-Pipeline** abgedeckt — ohne `SetWindowSubclass`,
  ohne Raw-Pointer-Hooks, ohne geleakte Hooks, ohne `unsafe impl Send`.
* Zusätzlich in der Zeilen-`TouchArea`: Back/Forward werden wie global
  behandelt, Rechtsklick öffnet das Zeilen-Kontextmenü, Strg/Shift
  Multi-/Bereichsauswahl bleibt intakt.

### B.2 Icons

* `IconStore` auf dem UI-Thread: dedupe von In-Flight-Requests, Cache-Hits
  werden synchron gestempelt, Misses gehen an einen dedizierten Icon-Worker-
  Thread (`kova-icons`, außerhalb des Tokio-Runtimes, serialisiert über
  `SHELL_ICON_LOCK`).
* Generic-Icons (Folder/File/Symlink/Drive/Unknown) werden beim Start
  synchron vorgeladen (Slots 0..=4), damit Zeilen nie icon-los sind.
* Sidebar (Home/Desktop/Documents/Downloads) und Drives erhalten echte
  Shell-Icons; Dateizeilen bekommen Erweiterungs-Icons (`.txt`, …) über den
  asynchronen Worker, EXE/LNK pro Pfad.
* **Bug gefunden und behoben**: `IconStore.next_id` wurde nach dem Preseed
  auf `8` gesetzt, während das Icon-Modell nur 5 Zeilen hatte — jede
  async-aufgelöste Icon-ID zeigte ins Leere (`icons[id]` außerhalb des
  Modells). Die ID wird jetzt direkt aus `model.row_count()` abgeleitet
  (ID == Zeilenindex, immer konsistent).
* Fallback-Kette: Shell-Icon → generisches Typ-Icon (Folder/File) → nichts;
  `FileListItem.is_dir` ermöglicht das „Open in New Tab“-Menü nur für Ordner.

### B.3 Kontextmenüs & Interaktion

* Zeilen-Kontextmenü: Open, Open in New Tab (nur Ordner), Rename, Copy Path,
  New Folder, Refresh. Umsetzung über Slint `ContextMenuArea` (natives
  Win32-Menü, per Automations-Test verifiziert).
* Kontextmenü für die leere Liste: New Folder, Refresh.
* Rechtsklick auf nicht ausgewählte Zeile wählt diese aus und erhält
  Mehrfachauswahl.
* `Open in New Tab`: neuer Tab wird aktiv, eigene Historie ab dem Zielort,
  sofortige Enumeration; `dispatch_open_in_new_tab` lehnt Nicht-Ordner ab.
* `Copy Path`: vollständiger Windows-Pfad in die Zwischenablage
  (`kova_platform_windows::clipboard`), Bestätigung in der Statuszeile.
* Fehler von Nutzeraktionen landen sichtbar in der Statuszeile
  (`show_action_error`) statt still zu versagen.

### B.4 Product-Quality UI

* Neues Design-Token-Set (`Theme`): ruhendere Oberflächen, dezente Border,
  klarer Pressed-State statt Vollflächen-Blau, `danger`-Token für später.
* Statuszeile: Status, Item-Count, Selected-Count, Loading-Indikator.
* Loading/Empty-State zentriert („Loading…“ / „This folder is empty“).
* Dialog: Backdrop blockiert jetzt jeden Klick hinter den Dialog
  (`dialog-blocker`), Escape schließt, Fokus + Select-All beim Öffnen.
* Header mit Sortier-Indikator in Akzentfarbe, Trennlinie, 1-px-Borders.
* Zeilenhöhe/Metriken zentral in `Metrics`; Icon-Größe 16px.
* Tabs: aktiver Tab mit Border+Akzent, Hover/Pressed-States, elide.

### B.5 Bereinigung (Debug-Instrumentierung entfernt)

* `debug_pointer`-Callback (Rust + Slint) entfernt.
* Temporäre `nav:`/`activate:`/`rename:`-Trace-Logs in `bridges.rs` entfernt.
* Icon-Timing-/Batch-Debug-Logs und `controller: snapshot accepted` entfernt.
* UTF-8-BOMs aus `main.rs`, `main.slint`, `shell_icons.rs` entfernt.
* **Encoding-Reparatur**: `main.slint` enthielt doppelt-kodierte UTF-8-Strings
  (Pfeile, `×`, Ellipse, Sortier-Pfeile). Sämtliche UI-Strings sind wieder
  korrekt (`←`, `→`, `↑`, `↻`, `×`, `▲`, `▼`, `…`) — die Toolbar-Glyphen
  waren vorher faktisch kaputt.

---

## C. Quality Gates

```text
cargo fmt --all -- --check:   PASS
cargo check --workspace --all-targets: PASS
cargo test --workspace:       PASS (32 passed, 1 ignored: perf baseline)
cargo clippy --workspace --all-targets -- -D warnings: PASS
cargo build --release:        PASS (MSVC, vcvars64, 29.7s)
```

Unit-Tests neu in M1: `item_and_selection_counts_track_active_tab`,
`file_list_rows_expose_generic_icon_and_dir_flag` (Fallback-IDs, `is_dir`,
Shell-Icon-Vorrang).

---

## D. Runtime-Verifikation (real, Release-Binary, UIA + SendInput)

Sandbox: `%TEMP%\kova-m1-final` mit Ordnern `M1Target`, `M1Back` (bzw.
`M1Created`/`M1Renamed`) und `note.txt`. Verifikation über UIA-Elementbaum,
Sende-Eingaben und Dateisystem/App-Log als Ground Truth.

| Test | Ergebnis | Evidence |
| --- | --- | --- |
| Open in New Tab (Rechtsklick → Kontextmenü → „Open in New Tab“) | PASS | natives Menü gefunden; Adresse `...\M1Target`; Log: `enumerate tab=TabId(2)`; Tab schließt → Adresse zurück, `tabs=1` |
| New Folder (Toolbar → Dialog → OK) | PASS | Ordner auf Disk erzeugt; Log `entries=3`; Tastatur-Dialog-Flow (Autofokus + Select-All) |
| Rename (Zeile wählen → F2 → Dialog → OK) | PASS | `M1Renamed` existiert, `M1Created` entfernt (Disk), gleiche Session wie New Folder |
| Icons in der Dateiliste | PASS | Pixel-Scan der Icon-Spalte (20×20 pro Zeile): 126 farbige Pixel je Zeile (Icon-Slots, Text beginnt erst bei x≈230) |
| Zeile Einzelklick-Auswahl | PASS | Statuszeile „1 selected“ |
| Zeile Doppelklick öffnen | PASS | Adresse wechselt in Ordner; Log `enumerate ...\<Ordner>` |
| Rechtsklick-Kontextmenü erscheint | PASS | natives Popup (`#32768`) via UIA gefunden |
| Tab schließen (×) | PASS | Adresse zurück auf Ausgangsordner, Log `tabs=1` |
| Maus Back/Forward (XBUTTON1/2) | **USER VERIFICATION** | siehe unten |

Hinweis zu Screenshots für den Nutzer: `.rivet_temp/m1_final_view.png`
(Hauptansicht) und `.rivet_temp/m1_dialog.png` (New-Folder-Dialog; Dialogbox
ist zentriert, OK-Button bei Client-X 536..615 — per Pixelanalyse verifiziert).

### D.1 XBUTTON1/2 — USER VERIFICATION

Zwei Synthese-Wege wurden getestet und lieferten in der Automation kein
Ereignis im Fenster:

1. `SendInput` mit `MOUSEEVENTF_XDOWN/XUP` und korrektem `mouseData`
   (XBUTTON1/2).
2. `PostMessageW(WM_XBUTTONDOWN/UP)` mit korrektem Encoding (Button im
   High-Word von wParam, Screen-Koordinaten in lParam).

Verifikation der Kette: winit 0.30.13 behandelt `WM_XBUTTONDOWN` und mappt
XBUTTON1/2 auf `MouseButton::Back/Forward`; Slint 1.13.1 mappt diese auf
`PointerEventButton::Back/Forward` (bereits früher verifiziert). Die App-Seite
(`NavTouchArea` + Zeilen-`TouchArea`) ruft bei beiden Buttons
`request_back()`/`request_forward()` auf.

**Bitte am echten Gerät prüfen:**

1. Kova starten, einen Ordner öffnen (Doppelklick), dann:
2. Maus-Back-Taste (XBUTTON1) → App muss einen Ordner zurückgehen.
3. Maus-Forward-Taste (XBUTTON2) → App muss wieder vorwärtsgehen.
4. Auch über Toolbar/Tabs/Sidebar/leerer Listenbereich testen.

Falls XBUTTON2 auf der echten Maus nicht funktioniert: Rückmeldung mit
„Back funktioniert / Forward funktioniert nicht“ genügt; dann ist der nächste
Schritt ein gezielter Fix in der Ereignis-Übergabe, keine neue Architektur.

---

## E. Bekannte Grenzen / Deferred

* Multi-Select-Renamen, Zwischenablage-Operationen (Copy/Cut/Paste),
  Drag-and-drop, Undo — weiterhin M2+.
* Kontextmenü-Icons und Tastatur-Navigation innerhalb des Menüs folgt dem
  nativen Win32-Verhalten (kein Custom-Styling).
* Icon-Worker resolved pro Key einmal pro Prozess; negative Ergebnisse werden
  gecacht (kein Retry-Thrash).
* Der Performance-Baseline-Test bleibt `ignored` (bewusst, langläufig).

---

## F. Fazit

M1-Ziele sind implementiert und mit realer Runtime-Evidence belegt. Die
verbleibende offene Verifikation ist ausschließlich die physische
Maus-Back/Forward-Prüfung (USER VERIFICATION), da synthetische X-Button-
Events in der Automations-Umgebung nicht zuverlässig ankommen — die
verwendete Slint-API ist verifiziert und der Code nutzt durchgängig die
normale Slint-Input-Pipeline.