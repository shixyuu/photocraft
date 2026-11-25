//! `cargo xtask android`: native PhotoCraft APK (arm64-v8a and x86_64).
//!
//! The Rust library is built with cargo-ndk. Gradle packages GameActivity around it and signs
//! with the debug keystore. Prerequisites and the runtime limits are in docs/android.md.

use std::path::{Path, PathBuf};
use std::process::Command;

const GRADLE: &str = "8.11.1";
const CARGO_NDK: &str = "4.1.2";

pub fn run(root: &Path, args: &[&str]) -> Result<(), String> {
    let mut abi = "all".to_string();
    let mut debug = false;
    let mut i = 0;
    while i < args.len() {
        match args[i] {
            "--abi" => {
                i += 1;
                abi = args.get(i).ok_or("--abi needs arm64-v8a, x86_64, or all")?.to_string();
            }
            "--debug" => debug = true,
            other => return Err(format!("unknown android argument `{other}`")),
        }
        i += 1;
    }
    let abis: Vec<&str> = match abi.as_str() {
        "all" => vec!["arm64-v8a", "x86_64"],
        "arm64-v8a" | "x86_64" => vec![abi.as_str()],
        other => return Err(format!("unknown abi `{other}` (arm64-v8a, x86_64, or all)")),
    };
    let sdk = sdk_root()?;
    let ndk = ndk_home(&sdk)?;
    ensure_rust_targets(&abis)?;
    ensure_cargo_ndk()?;
    let jni = root.join("packaging/android/app/src/main/jniLibs");
    let _ = std::fs::remove_dir_all(&jni);
    let profile = if debug { "dev" } else { "release" };
    let mut ndk_cmd = Command::new("cargo");
    ndk_cmd.arg("ndk").current_dir(root).env("ANDROID_NDK_HOME", &ndk).env("ANDROID_SDK_ROOT", &sdk);
    for name in &abis {
        ndk_cmd.arg("-t").arg(name);
    }
    ndk_cmd.args(["-o", jni.to_str().ok_or("jniLibs path is not utf-8")?]);
    ndk_cmd.args(["build", "-p", "photocraft-android"]);
    if !debug {
        ndk_cmd.arg("--release");
    }
    ndk_cmd.env("CARGO_BUILD_JOBS", std::env::var("CARGO_BUILD_JOBS").unwrap_or_else(|_| "2".into()));
    crate::run(ndk_cmd, "cargo ndk build -p photocraft-android")?;
    for name in &abis {
        let so = jni.join(name).join("libphotocraft.so");
        if !so.is_file() {
            return Err(format!("missing {}", so.display()));
        }
    }
    let gradle = ensure_gradle()?;
    let props = root.join("packaging/android/local.properties");
    std::fs::write(&props, format!("sdk.dir={}\n", sdk.display())).map_err(|e| e.to_string())?;
    let task = if debug { "assembleDebug" } else { "assembleRelease" };
    let mut gradle_cmd = Command::new(&gradle);
    gradle_cmd.current_dir(root.join("packaging/android")).arg(task).env("ANDROID_SDK_ROOT", &sdk).env("ANDROID_HOME", &sdk);
    crate::run(gradle_cmd, &format!("gradle {task}"))?;
    let apk_name = if debug { "app-debug.apk" } else { "app-release.apk" };
    let built = root.join("packaging/android/app/build/outputs/apk").join(if debug { "debug" } else { "release" }).join(apk_name);
    if !built.is_file() {
        return Err(format!("gradle did not write {}", built.display()));
    }
    let out_dir = root.join("target/android");
    std::fs::create_dir_all(&out_dir).map_err(|e| e.to_string())?;
    let abi_label = if abis.len() == 2 { "all".to_string() } else { abis[0].to_string() };
    let dest = out_dir.join(format!("photocraft-{profile}-{abi_label}.apk"));
    std::fs::copy(&built, &dest).map_err(|e| e.to_string())?;
    eprintln!("APK {}", dest.display());
    Ok(())
}

fn sdk_root() -> Result<PathBuf, String> {
    for key in ["ANDROID_SDK_ROOT", "ANDROID_HOME"] {
        if let Some(value) = std::env::var_os(key) {
            let path = PathBuf::from(value);
            if path.join("platforms").is_dir() {
                return Ok(path);
            }
        }
    }
    let home = std::env::var_os("HOME").map(PathBuf::from).ok_or("HOME is unset")?;
    let path = home.join("android-sdk");
    if path.join("platforms").is_dir() {
        return Ok(path);
    }
    Err("Android SDK not found. Set ANDROID_SDK_ROOT (docs/android.md).".into())
}

fn ndk_home(sdk: &Path) -> Result<PathBuf, String> {
    if let Some(value) = std::env::var_os("ANDROID_NDK_HOME") {
        let path = PathBuf::from(value);
        if path.join("toolchains").is_dir() || path.join("ndk-build").is_file() {
            return Ok(path);
        }
    }
    let root = sdk.join("ndk");
    let mut versions = std::fs::read_dir(&root).map_err(|e| format!("{}: {e}", root.display()))?.filter_map(|e| e.ok()).map(|e| e.path()).collect::<Vec<_>>();
    versions.sort();
    versions.pop().filter(|p| p.is_dir()).ok_or_else(|| format!("no NDK under {}", root.display()))
}

fn ensure_rust_targets(abis: &[&str]) -> Result<(), String> {
    for abi in abis {
        let triple = match *abi {
            "arm64-v8a" => "aarch64-linux-android",
            "x86_64" => "x86_64-linux-android",
            other => return Err(format!("no rust triple for {other}")),
        };
        let listed = Command::new("rustup").args(["target", "list", "--installed"]).output().map_err(|e| format!("rustup: {e}"))?;
        let text = String::from_utf8_lossy(&listed.stdout);
        if text.lines().any(|line| line.trim() == triple) {
            continue;
        }
        let mut add = Command::new("rustup");
        add.args(["target", "add", triple]);
        crate::run(add, &format!("rustup target add {triple}"))?;
    }
    Ok(())
}

fn ensure_cargo_ndk() -> Result<(), String> {
    if Command::new("cargo").args(["ndk", "--version"]).output().map(|o| o.status.success()).unwrap_or(false) {
        return Ok(());
    }
    eprintln!("installing cargo-ndk {CARGO_NDK}");
    let mut cmd = Command::new("cargo");
    cmd.args(["install", "cargo-ndk", "--locked", "--version", CARGO_NDK]);
    crate::run(cmd, "cargo install cargo-ndk")
}

fn ensure_gradle() -> Result<PathBuf, String> {
    if let Some(path) = which("gradle") {
        return Ok(path);
    }
    let home = std::env::var_os("HOME").map(PathBuf::from).ok_or("HOME is unset")?;
    let dir = home.join(format!(".cache/photocraft-gradle-{GRADLE}"));
    let bin = dir.join(format!("gradle-{GRADLE}/bin/gradle"));
    if bin.is_file() {
        return Ok(bin);
    }
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let url = format!("https://services.gradle.org/distributions/gradle-{GRADLE}-bin.zip");
    let zip = dir.join("gradle.zip");
    eprintln!("downloading {url}");
    let mut curl = Command::new("curl");
    curl.args(["-fsSL", "-o"]).arg(&zip).arg(&url);
    crate::run(curl, "curl gradle")?;
    let mut unzip = Command::new("unzip");
    unzip.arg("-q").arg(&zip).arg("-d").arg(&dir);
    crate::run(unzip, "unzip gradle")?;
    if bin.is_file() { Ok(bin) } else { Err(format!("gradle binary missing at {}", bin.display())) }
}

fn which(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}
