# Kova demo

[Watch or download the 28-second MP4](media/kova-demo.mp4)

The README contains an animated GIF version of the same recording, with a link
to the H.264 MP4. Both show the real release-mode Windows application, operated
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
