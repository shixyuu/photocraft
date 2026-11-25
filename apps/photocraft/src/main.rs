//! PhotoCraft desktop binary.
//!
//! Release builds on Windows are GUI-subsystem apps, so launching from the Start Menu or Explorer
//! doesn't open a console window. (`--help`, `--version` and usage errors then only show when
//! redirected.)
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

fn main() -> eframe::Result {
    photocraft::desktop_main()
}
