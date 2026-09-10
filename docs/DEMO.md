# Product captures

The README uses unedited window captures of the 0.2 release candidate taken on
10 September 2026 on an interactive Windows Server 2025 hosted desktop at 100%
scaling (984 × 680 physical window pixels). The executable comes from the verified
[0.2.0 package](https://github.com/cubiix3/Kova-File-Manager/actions/runs/34473650758),
application source `ea92807`. Files and preferences use isolated fixtures on a
temporary SUBST drive.

[View/conflict captures and performance](https://github.com/cubiix3/Kova-File-Manager/actions/runs/34475306852)
and the [verified native-menu capture](https://github.com/cubiix3/Kova-File-Manager/actions/runs/34476071198)
retain their raw logs and images as workflow artifacts.

- `images/daily-driver-details.png`: tabs, readable search/filter controls and details.
- `images/daily-driver-inspector.png`: text preview and Copy Path.
- `images/daily-driver-gallery.png`: gallery and inspector with copyable metadata.
- `images/daily-driver-native.png`: the real Windows Shell context menu.
- `images/daily-driver-home.png`: recent folders, favorites and drives.
- `images/daily-driver-conflict.png`: incoming/existing comparison and explicit choices.

`runtime-window.ps1` captures pixels directly from the specified test window.
No generated UI, recoloring, image editing or product framing is applied.

Older `product-*`, `current-*`, `nextgen-*` images and videos preserve earlier
milestones; their interfaces and feature scope are historical.
