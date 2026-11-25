# PhotoCraft on Android

PhotoCraft on Android is the desktop editor: the same egui shell, panels, tools, and PSD IO,
drawn by wgpu. It is a native library loaded by [GameActivity](https://developer.android.com/games/agdk/game-activity),
not a page in a WebView. The window is the activity surface. Suspend drops the wgpu surface and
resume creates it again (eframe already does this). Vulkan is the preferred backend; GLES is used
only when no Vulkan adapter can present.

The UI is the desktop layout. It is meant for a tablet, a foldable, or a desktop-style Android
window with a keyboard and a mouse. Touch moves the same pointers. A stylus reports pressure
through the motion event (tilt is not forwarded by this winit revision). Text fields open the
soft keyboard through GameActivity's text input.

## Build

One command, from the repository root:

```sh
cargo xtask android
```

That builds `arm64-v8a` and `x86_64` release libraries, packages them, and signs the APK with the
debug keystore. The APK is `target/android/photocraft-release-all.apk`.

```sh
cargo xtask android --abi x86_64    # emulator
cargo xtask android --abi arm64-v8a # devices
cargo xtask android --debug         # unoptimized, still signed
```

Install with `adb install -r target/android/photocraft-release-all.apk`.

## Prerequisites

| Tool | Version used here |
|---|---|
| Rust | `rust-toolchain.toml` (1.100.0), targets `aarch64-linux-android` and `x86_64-linux-android` |
| Android SDK | platform 35, build-tools 35.0.0 |
| Android NDK | r27 (`27.2.12479018`) |
| JDK | 17 or newer |
| cargo-ndk | 4.1.2 (installed by the xtask if it is missing) |
| Gradle | 8.11.1 (downloaded by the xtask if `gradle` is not on `PATH`) |

```sh
rustup target add aarch64-linux-android x86_64-linux-android
```

Set `ANDROID_SDK_ROOT` to the SDK. The xtask also looks at `ANDROID_HOME` and `~/android-sdk`.
It uses the newest directory under `$SDK/ndk` unless `ANDROID_NDK_HOME` is set. Accept the SDK
licenses once (`sdkmanager --licenses`) before the first Gradle run.

The SDK packages this build expects:

```text
platforms;android-35
build-tools;35.0.0
ndk;27.2.12479018
platform-tools
```

An emulator image for the x86_64 APK: `system-images;android-35;google_apis;x86_64`.

## What the activity does

`packaging/android` is a small Gradle project. `PhotoCraftActivity` extends GameActivity and
loads `libphotocraft.so`. Rust and the activity share `files/bridge`:

- **Open** uses the system document picker (Storage Access Framework). The bytes are copied into
  the app's files directory and the editor opens that path.
- **Save** asks for a document, lets the editor write a normal file, then copies that file to the
  chosen URI once the write has finished.
- **Open with / share** an image onto a cold start copies it and opens it.
- **Clipboard** text and images go through the system clipboard.
- **Links** open in the system viewer.

The first launch also writes `files/imports/sample.png` and opens it, so the window comes up on
a document. The selected wgpu adapter is logged (`PhotoCraft` logcat tag) and written to
`files/gpu-adapter.txt` (`adb shell run-as ai.storyteller.photocraft cat files/gpu-adapter.txt`
on a debug install).

Settings live in the app's internal files directory.

## Limits

These desktop paths are not available on Android:

- A second process handing files to the one already running, and the TCP control server's
  single-instance lock, are not started.
- Self-update does not run. Help › Check for Updates has no checker: the desktop TLS verifier
  is not initialized on Android.
- Printing, the Linux color portal, pen tilt, and fontconfig are absent. Installed fonts are read
  from `/system/fonts` (and the product font directories) when those directories exist. The UI's
  own fonts are the ones the desktop build embeds.
- arboard has no Android backend. Copy and paste use the activity bridge instead.
- The file dialogs are the system document UI, not the desktop portal. A folder pick copies the
  top level of the chosen tree into app storage.
- High-density phones scale the desktop layout up. Panels scroll; a large window is the
  comfortable size.

`WGPU_BACKEND=gl` still forces GLES. The default automatic choice asks for Vulkan first.
