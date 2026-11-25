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
    let profile = if debug { "debug" } else { "release" };
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
    // Joined with .cargo/config.toml. The final cdylib link is what sets LOAD alignment.
    let page = "-C link-arg=-Wl,-z,max-page-size=16384";
    ndk_cmd.env("CARGO_TARGET_AARCH64_LINUX_ANDROID_RUSTFLAGS", page);
    ndk_cmd.env("CARGO_TARGET_X86_64_LINUX_ANDROID_RUSTFLAGS", page);
    ndk_cmd.env("LDFLAGS", "-Wl,-z,max-page-size=16384");
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
    let out_dir = root.join("packaging/android/app/build/outputs/apk").join(if debug { "debug" } else { "release" });
    let built = std::fs::read_dir(&out_dir)
        .map_err(|e| format!("{}: {e}", out_dir.display()))?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "apk"))
        .collect::<Vec<_>>();
    if built.is_empty() {
        return Err(format!("gradle did not write an apk in {}", out_dir.display()));
    }
    let dest_dir = root.join("target/android");
    std::fs::create_dir_all(&dest_dir).map_err(|e| e.to_string())?;
    for apk in built {
        let signed = align_and_sign(&sdk, &apk)?;
        let dest = dest_dir.join(published_name(&apk, profile));
        std::fs::copy(&signed, &dest).map_err(|e| e.to_string())?;
        eprintln!("APK {}", dest.display());
    }
    Ok(())
}

/// `app-arm64-v8a-release.apk` becomes `photocraft-release-arm64-v8a.apk`.
fn published_name(apk: &Path, profile: &str) -> String {
    let stem = apk.file_stem().and_then(|s| s.to_str()).unwrap_or("app");
    let mut label = stem.strip_prefix("app-").unwrap_or(stem).to_string();
    for suffix in ["-release", "-debug"] {
        if let Some(stripped) = label.strip_suffix(suffix) {
            label = stripped.to_string();
        }
    }
    format!("photocraft-{profile}-{label}.apk")
}

/// 16 KB-align uncompressed native libraries, then sign. Alignment must be the last
/// byte change before the signature; signing first and aligning after invalidates it.
fn align_and_sign(sdk: &Path, apk: &Path) -> Result<PathBuf, String> {
    let tools = build_tools(sdk)?;
    let zipalign = tools.join("zipalign");
    let apksigner = tools.join("apksigner");
    let aligned = apk.with_extension("aligned.apk");
    let mut align = Command::new(&zipalign);
    align.arg("-f").arg("-P").arg("16").arg("4").arg(apk).arg(&aligned);
    crate::run(align, "zipalign -P 16")?;
    let keystore = debug_keystore()?;
    let mut sign = Command::new(&apksigner);
    sign.arg("sign")
        .arg("--ks")
        .arg(&keystore)
        .arg("--ks-key-alias")
        .arg("androiddebugkey")
        .arg("--ks-pass")
        .arg("pass:android")
        .arg("--key-pass")
        .arg("pass:android")
        .arg("--v1-signing-enabled")
        .arg("true")
        .arg("--v2-signing-enabled")
        .arg("true")
        .arg("--v3-signing-enabled")
        .arg("true")
        .arg("--v4-signing-enabled")
        .arg("false")
        .arg(&aligned);
    crate::run(sign, "apksigner sign")?;
    let mut check = Command::new(&zipalign);
    check.arg("-c").arg("-P").arg("16").arg("4").arg(&aligned);
    crate::run(check, "zipalign -c -P 16")?;
    let mut verify = Command::new(&apksigner);
    verify.arg("verify").arg("--verbose").arg("--min-sdk-version").arg("24").arg("--max-sdk-version").arg("35").arg(&aligned);
    crate::run(verify, "apksigner verify")?;
    Ok(aligned)
}

fn build_tools(sdk: &Path) -> Result<PathBuf, String> {
    let root = sdk.join("build-tools");
    let mut versions = std::fs::read_dir(&root).map_err(|e| format!("{}: {e}", root.display()))?.filter_map(|e| e.ok()).map(|e| e.path()).collect::<Vec<_>>();
    versions.sort();
    versions.pop().filter(|p| p.join("apksigner").is_file()).ok_or_else(|| format!("no build-tools under {}", root.display()))
}

fn debug_keystore() -> Result<PathBuf, String> {
    let home = std::env::var_os("HOME").map(PathBuf::from).ok_or("HOME is unset")?;
    let path = home.join(".android/debug.keystore");
    if path.is_file() {
        return Ok(path);
    }
    std::fs::create_dir_all(path.parent().unwrap_or(Path::new("."))).map_err(|e| e.to_string())?;
    let mut keytool = Command::new("keytool");
    keytool
        .arg("-genkeypair")
        .arg("-keystore")
        .arg(&path)
        .arg("-storepass")
        .arg("android")
        .arg("-keypass")
        .arg("android")
        .arg("-alias")
        .arg("androiddebugkey")
        .arg("-keyalg")
        .arg("RSA")
        .arg("-keysize")
        .arg("2048")
        .arg("-validity")
        .arg("10000")
        .arg("-dname")
        .arg("CN=Android Debug,O=Android,C=US");
    crate::run(keytool, "keytool debug keystore")?;
    Ok(path)
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
