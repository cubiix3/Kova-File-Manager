# Kova identity

Original Kova mark: a two-tone cyan K on a rounded ink tile. The lower diagonal
fold gives the mark depth while the silhouette remains readable at 16 pixels.
It uses Kova's own geometry; no Files or other application's branding is reused.
These assets use the repository's license.

- `kova.svg`: scalable logo.
- `kova-mark.svg`: standalone K geometry for the approved caption design.
- `kova.png`: 256px transparent preview and runtime window/taskbar icon.
- `kova.ico`: embedded executable icon, 16/24/32/48/64/128/256px, 32-bit RGBA.

Regenerate all three together on Windows:

```powershell
.\scripts\generate-app-icon.ps1
```

`build.rs` embeds the ICO and product name in the executable's Windows resources.
Slint sets the window icon independently, so the running app and Explorer both
show the Kova mark. The caption uses the standalone `kova-mark.svg` variant.
