//! Android `android_main`. The activity loads this cdylib; the editor itself is `photocraft`.
//!
//! `#[unsafe(no_mangle)]` is required by android-activity (Rust ABI, not C). There is no unsafe block.

#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
fn android_main(app: winit::platform::android::activity::AndroidApp) {
    if let Err(error) = photocraft::android_entry(app) {
        eprintln!("PhotoCraft stopped: {error}");
    }
}

/// Desktop workspace checks do not link the editor into this crate.
#[cfg(not(target_os = "android"))]
pub fn android_entry_is_built_for_android_only() {}
