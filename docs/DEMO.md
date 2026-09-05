# Kova videos

## Product teaser

[Watch / download with sound](media/Kova-Product-Teaser.mp4?raw=true) ?
[Silent version](media/Kova-Product-Teaser-Silent.mp4?raw=true)

The 15-second teaser uses new recordings of the real Kova application: tab and
folder navigation, Home, the native Windows Shell menu with the installed 7-Zip
extension, three-file selection and Copy/Paste, and an image-preview hero view.
The copied demonstration files were checked against their originals.

Camera movement, viewport masks, typography and the existing Kova logo are
composited around genuine UI footage. The soundtrack was synthesized for this
teaser, including a music bed, whooshes, clicks and a logo impact; no third-party
audio samples were used. Native Shell menu labels follow the Windows/provider
languages. Kova's own controls are English.

Both exports are 1920 ? 1080 H.264 MP4 at 60 FPS, with fast-start metadata. Main
and Silent contain the same video payload; Main adds stereo AAC audio. All 900
decoded frames were checked for black frames, adjacent duplicates and isolated
luminance spikes, and the final timeline was reviewed visually.

## Longer application demo

[Download the 28-second MP4](media/kova-demo.mp4?raw=true)

An [animated GIF version](media/kova-demo.gif) is also available. Both show the real release-mode Windows application, operated
with mouse and keyboard input. No interface screens are generated or composited.

The sequence covers Home and drive capacity, switching between two tabs, a PNG
preview, an animated GIF playing in the inspector, and the native Windows Shell
context menu. The menu uses the recording machine's Windows/provider languages;
Kova's own controls are English.

Captured on Windows 11 at 1280 × 800, 15 frames per second, without audio, using
demonstration files and an isolated view-preferences directory. User documents
and clipboard content are not part of the recording. The GIF is reduced to
960 × 600 for the README; the MP4 retains the original capture resolution.

This footage shows the English development build. The existing v0.1.0 installer
predates that change and still has mixed German/English labels. Build the current
source to use the new interface until a newer Windows package is published.

To refresh the recording, build with `scripts/cargo-msvc.ps1 build --release`,
use a clean demonstration folder, and record the application window for 28
seconds. Keep Shell popups in the capture, review every segment, and verify
that animation actually advances. Export H.264/yuv420p with fast-start metadata
as `docs/media/kova-demo.mp4` and a matching GIF as `docs/media/kova-demo.gif`.
Refresh the static screenshots alongside the recording.
