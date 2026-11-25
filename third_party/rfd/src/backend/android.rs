//! Android file and message dialogs.
//!
//! rfd's other backends need a desktop portal. On Android the activity watches a directory
//! ([`set_bridge_dir`]) for `request.json`, shows the system UI, and writes `response.json`.
//! Opened documents are copied to real files; a save returns a path the app writes, which the
//! activity then copies to the document the person chose.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::backend::{
    AsyncFilePickerDialogImpl, AsyncFileSaveDialogImpl, AsyncFolderPickerDialogImpl, AsyncMessageDialogImpl, DialogFutureType, FilePickerDialogImpl,
    FileSaveDialogImpl, FolderPickerDialogImpl, MessageDialogImpl,
};
use crate::file_handle::FileHandle;
use crate::{FileDialog, MessageButtons, MessageDialog, MessageDialogResult};

static BRIDGE: Mutex<Option<PathBuf>> = Mutex::new(None);
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// Where the activity reads `request.json` and writes `response.json`.
pub fn set_bridge_dir(dir: PathBuf) {
    let _ = std::fs::create_dir_all(&dir);
    *BRIDGE.lock().unwrap_or_else(PoisonError::into_inner) = Some(dir);
}

fn bridge_dir() -> Result<PathBuf, String> {
    BRIDGE.lock().unwrap_or_else(PoisonError::into_inner).clone().ok_or_else(|| "Android file bridge is not configured".to_string())
}

/// Send one request and wait for the activity's response with the same `id`.
pub fn exchange(mut body: Value, timeout: Duration) -> Result<Value, String> {
    let dir = bridge_dir()?;
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    body["id"] = json!(id);
    let request = dir.join("request.json");
    let response = dir.join("response.json");
    let _ = std::fs::remove_file(&response);
    let tmp = dir.join("request.json.tmp");
    std::fs::write(&tmp, body.to_string()).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &request).map_err(|e| e.to_string())?;
    let started = Instant::now();
    while started.elapsed() < timeout {
        if let Some(value) = read_response(&response, id) {
            let _ = std::fs::remove_file(&response);
            return Ok(value);
        }
        std::thread::sleep(Duration::from_millis(40));
    }
    let _ = std::fs::remove_file(&request);
    Err("Android file bridge timed out".into())
}

fn read_response(path: &Path, id: u64) -> Option<Value> {
    let text = std::fs::read_to_string(path).ok()?;
    let value: Value = serde_json::from_str(&text).ok()?;
    (value.get("id").and_then(Value::as_u64) == Some(id)).then_some(value)
}

fn paths_from(value: &Value) -> Option<Vec<PathBuf>> {
    if value.get("cancel").and_then(Value::as_bool) == Some(true) {
        return None;
    }
    let paths: Vec<PathBuf> = value.get("paths")?.as_array()?.iter().filter_map(|v| v.as_str().map(PathBuf::from)).filter(|p| !p.as_os_str().is_empty()).collect();
    (!paths.is_empty()).then_some(paths)
}

fn extensions(dialog: &FileDialog) -> Vec<String> {
    dialog.filters.iter().flat_map(|filter| filter.extensions.iter().cloned()).collect()
}

fn one_file(dialog: FileDialog, multiple: bool, directory: bool, save: bool) -> Option<Vec<PathBuf>> {
    let mime = if save { mime_for(dialog.file_name.as_deref(), &extensions(&dialog)) } else { "image/*".to_string() };
    let body = json!({
        "op": if directory { "folder" } else if save { "save" } else { "open" },
        "multiple": multiple,
        "name": dialog.file_name.clone().unwrap_or_default(),
        "mime": mime,
        "filters": extensions(&dialog),
        "title": dialog.title.clone().unwrap_or_default(),
    });
    let value = exchange(body, Duration::from_secs(180)).ok()?;
    paths_from(&value)
}

fn mime_for(name: Option<&str>, extensions: &[String]) -> String {
    let ext = name.and_then(|n| Path::new(n).extension().and_then(|e| e.to_str())).or_else(|| extensions.first().map(String::as_str)).unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "psd" | "psb" => "image/vnd.adobe.photoshop",
        "tif" | "tiff" => "image/tiff",
        _ => "application/octet-stream",
    }
    .to_string()
}

fn ready<T: Send + 'static>(value: T) -> DialogFutureType<T> {
    Box::pin(std::future::ready(value))
}

fn first_path(paths: Option<Vec<PathBuf>>) -> Option<PathBuf> {
    paths.and_then(|mut paths| if paths.is_empty() { None } else { Some(paths.remove(0)) })
}

impl FilePickerDialogImpl for FileDialog {
    fn pick_file(self) -> Option<PathBuf> {
        first_path(one_file(self, false, false, false))
    }

    fn pick_files(self) -> Option<Vec<PathBuf>> {
        one_file(self, true, false, false)
    }
}

impl FileSaveDialogImpl for FileDialog {
    fn save_file(self) -> Option<PathBuf> {
        first_path(one_file(self, false, false, true))
    }
}

impl FolderPickerDialogImpl for FileDialog {
    fn pick_folder(self) -> Option<PathBuf> {
        first_path(one_file(self, false, true, false))
    }

    fn pick_folders(self) -> Option<Vec<PathBuf>> {
        one_file(self, true, true, false)
    }
}

impl AsyncFilePickerDialogImpl for FileDialog {
    fn pick_file_async(self) -> DialogFutureType<Option<FileHandle>> {
        ready(FilePickerDialogImpl::pick_file(self).map(FileHandle::wrap))
    }

    fn pick_files_async(self) -> DialogFutureType<Option<Vec<FileHandle>>> {
        ready(FilePickerDialogImpl::pick_files(self).map(|paths| paths.into_iter().map(FileHandle::wrap).collect()))
    }
}

impl AsyncFileSaveDialogImpl for FileDialog {
    fn save_file_async(self) -> DialogFutureType<Option<FileHandle>> {
        ready(FileSaveDialogImpl::save_file(self).map(FileHandle::wrap))
    }
}

impl AsyncFolderPickerDialogImpl for FileDialog {
    fn pick_folder_async(self) -> DialogFutureType<Option<FileHandle>> {
        ready(FolderPickerDialogImpl::pick_folder(self).map(FileHandle::wrap))
    }

    fn pick_folders_async(self) -> DialogFutureType<Option<Vec<FileHandle>>> {
        ready(FolderPickerDialogImpl::pick_folders(self).map(|paths| paths.into_iter().map(FileHandle::wrap).collect()))
    }
}

impl MessageDialogImpl for MessageDialog {
    fn show(self) -> MessageDialogResult {
        let buttons = match &self.buttons {
            MessageButtons::Ok | MessageButtons::OkCustom(_) => "ok",
            MessageButtons::OkCancel | MessageButtons::OkCancelCustom(_, _) => "ok-cancel",
            MessageButtons::YesNo => "yes-no",
            MessageButtons::YesNoCancel | MessageButtons::YesNoCancelCustom(_, _, _) => "yes-no-cancel",
        };
        let body = json!({
            "op": "message",
            "title": self.title,
            "text": self.description,
            "buttons": buttons,
        });
        let Ok(value) = exchange(body, Duration::from_secs(180)) else {
            return MessageDialogResult::Cancel;
        };
        if value.get("cancel").and_then(Value::as_bool) == Some(true) {
            return MessageDialogResult::Cancel;
        }
        match value.get("text").and_then(Value::as_str).unwrap_or("ok") {
            "yes" => MessageDialogResult::Yes,
            "no" => MessageDialogResult::No,
            "cancel" => MessageDialogResult::Cancel,
            _ => MessageDialogResult::Ok,
        }
    }
}

impl AsyncMessageDialogImpl for MessageDialog {
    fn show_async(self) -> DialogFutureType<MessageDialogResult> {
        ready(MessageDialogImpl::show(self))
    }
}
