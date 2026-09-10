# Regional formatting and translation

Kova currently ships English interface text. Windows Shell menus use the Windows
language. `kova-platform-windows::formatting` uses the user's Windows regional
settings for dates, grouped metadata numbers and decimal byte units. Sorting and
size filters compare raw values; display strings never become comparison keys.
Binary byte units are explicitly labeled KiB, MiB, GiB and TiB.

Static UI labels are marked with Slint's `@tr` macro, including plural search
counts. Translations must not change stored paths, virtual-location keys, search
syntax or command identifiers. Session persistence stores those data independently
of displayed text. Shared UI controls and extracted search/inspector/operation
components provide the boundary for an English/German catalog.

A complete German UI still requires catalogs, translation of Rust-generated
status/metadata labels and layout review of longer strings. No incomplete language
switch is exposed. Follow [Slint's translation workflow](https://docs.slint.dev/latest/docs/slint/guide/development/translations/)
with the version pinned by Cargo.lock when adding catalogs.
