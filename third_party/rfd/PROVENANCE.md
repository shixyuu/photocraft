# rfd 0.17.2

This directory is [rfd](https://github.com/PolyMeilex/rfd) 0.17.2 (MIT, `LICENSE`), the file-dialog
crate PhotoCraft already depends on, plus an Android backend.

Upstream 0.17.2 has no Android implementation, so the crate does not compile for
`target_os = "android"`. The additions are:

- `src/backend/android.rs` — dialogs and a small request/response bridge the Android activity
  fulfils with the Storage Access Framework. No `unsafe`.
- `src/lib.rs` — re-exports that bridge as `rfd::android_bridge`.
- `build.rs` — Android does not need the GTK or XDG Portal feature.
- `Cargo.toml` — `serde_json` on Android, for the bridge messages.

The workspace `[patch.crates-io]` points `rfd` here so desktop and Android share one copy.
Desktop backends are the upstream files, unchanged.
