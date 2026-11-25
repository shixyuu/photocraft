//! Android launch: app storage, the file-dialog bridge, a sample image, and the wgpu adapter note.
//!
//! The activity (packaging/android) fulfils [`rfd::android_bridge`] requests. This module does not
//! call JNI.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock, PoisonError};
use std::time::Duration;

use winit::platform::android::activity::AndroidApp;

static APP: Mutex<Option<AndroidApp>> = Mutex::new(None);
static NOTE_DIR: OnceLock<PathBuf> = OnceLock::new();
static MEM_TEXT: Mutex<String> = Mutex::new(String::new());
static MEM_IMAGE: Mutex<Option<(u32, u32, Vec<u8>)>> = Mutex::new(None);

/// Remember the activity for eframe, point settings and dialogs at app storage, and open the
/// sample image (or a document another app shared into the inbox).
pub fn entry(app: AndroidApp) -> eframe::Result {
    let data = app.internal_data_path().unwrap_or_else(|| PathBuf::from("/data/data/ai.storyteller.photocraft/files"));
    let _ = std::fs::create_dir_all(&data);
    crate::app_dirs::set_override(data.clone());
    let _ = NOTE_DIR.set(data.clone());
    let bridge = data.join("bridge");
    rfd::android_bridge::set_bridge_dir(bridge);
    *APP.lock().unwrap_or_else(PoisonError::into_inner) = Some(app);
    let sample = write_sample(&data);
    let open = inbox_path(&data).or(sample);
    let args = open.map(|path| vec![OsString::from(path.as_os_str())]);
    super::run_with_args(args)
}

pub fn take_app() -> Option<AndroidApp> {
    APP.lock().unwrap_or_else(PoisonError::into_inner).take()
}

/// Write the adapter eframe selected so logcat and `adb run-as` can both see it.
pub fn record_adapter(info: &eframe::wgpu::AdapterInfo) {
    let line = format!(
        "name={}\nbackend={:?}\ndevice_type={:?}\nvendor={:#x}\ndevice={:#x}\ndriver={}\ndriver_info={}\n",
        info.name, info.backend, info.device_type, info.vendor, info.device, info.driver, info.driver_info
    );
    log::info!("wgpu adapter selected:\n{line}");
    if let Some(dir) = NOTE_DIR.get() {
        let _ = std::fs::write(dir.join("gpu-adapter.txt"), &line);
    }
}

pub fn open_url(url: &str) -> Result<(), String> {
    let value = rfd::android_bridge::exchange(serde_json::json!({"op": "open-url", "text": url}), Duration::from_secs(8))?;
    bridge_error(&value)
}

pub fn get_clipboard_text() -> Result<Option<String>, String> {
    match rfd::android_bridge::exchange(serde_json::json!({"op": "clipboard-get-text"}), Duration::from_secs(4)) {
        Ok(value) => {
            bridge_error(&value)?;
            Ok(value.get("text").and_then(|v| v.as_str()).map(str::to_string).filter(|s| !s.is_empty()))
        }
        Err(_) => {
            let text = MEM_TEXT.lock().unwrap_or_else(PoisonError::into_inner).clone();
            Ok((!text.is_empty()).then_some(text))
        }
    }
}

pub fn set_clipboard_image(w: u32, h: u32, px: &[u8]) -> Result<(), String> {
    *MEM_IMAGE.lock().unwrap_or_else(PoisonError::into_inner) = Some((w, h, px.to_vec()));
    let Some(dir) = NOTE_DIR.get() else { return Ok(()) };
    let path = dir.join("bridge/clipboard.png");
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    write_rgba_png(&path, w, h, px)?;
    let value = rfd::android_bridge::exchange(serde_json::json!({"op": "clipboard-set-image", "path": path.display().to_string()}), Duration::from_secs(4))?;
    bridge_error(&value)
}

pub fn get_clipboard_image() -> Result<Option<(u32, u32, Vec<u8>)>, String> {
    match rfd::android_bridge::exchange(serde_json::json!({"op": "clipboard-get-image"}), Duration::from_secs(4)) {
        Ok(value) => {
            bridge_error(&value)?;
            let Some(path) = value.get("path").and_then(|v| v.as_str()) else {
                return Ok(MEM_IMAGE.lock().unwrap_or_else(PoisonError::into_inner).clone());
            };
            read_rgba_png(Path::new(path)).map(Some)
        }
        Err(_) => Ok(MEM_IMAGE.lock().unwrap_or_else(PoisonError::into_inner).clone()),
    }
}

fn bridge_error(value: &serde_json::Value) -> Result<(), String> {
    match value.get("error").and_then(|v| v.as_str()) {
        Some(error) if !error.is_empty() => Err(error.to_string()),
        _ => Ok(()),
    }
}

fn write_sample(data: &Path) -> Option<PathBuf> {
    let path = data.join("imports/sample.png");
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if !path.is_file() {
        std::fs::write(&path, crate::sample_png::sample_png()).ok()?;
    }
    path.is_file().then_some(path)
}

fn inbox_path(data: &Path) -> Option<PathBuf> {
    let text = std::fs::read_to_string(data.join("inbox/launch.txt")).ok()?;
    let path = PathBuf::from(text.trim());
    path.is_file().then_some(path)
}

fn write_rgba_png(path: &Path, w: u32, h: u32, px: &[u8]) -> Result<(), String> {
    let file = std::fs::File::create(path).map_err(|e| e.to_string())?;
    let mut encoder = png::Encoder::new(file, w, h);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
    writer.write_image_data(px).map_err(|e| e.to_string())
}

fn read_rgba_png(path: &Path) -> Result<(u32, u32, Vec<u8>), String> {
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut decoder = png::Decoder::new(file);
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info().map_err(|e| e.to_string())?;
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).map_err(|e| e.to_string())?;
    buf.truncate(info.buffer_size());
    let (w, h) = reader.info().size();
    if info.color_type != png::ColorType::Rgba {
        return Err(format!("clipboard image is {:?}, not RGBA", info.color_type));
    }
    Ok((w, h, buf))
}
