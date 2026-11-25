#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

// PhotoCraft as a library. The desktop binary and the Android cdylib both call into here.
include!("desktop_main.rs");
