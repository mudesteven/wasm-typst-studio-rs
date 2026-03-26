use crate::models::ProjectFile;

/// Sync status for a file
#[derive(Clone, Debug, PartialEq)]
pub enum SyncStatus {
    Synced,
    ModifiedLocally,
    ModifiedRemotely,
    Conflict,
    Syncing,
}

/// Trait for local filesystem sync (File System Access API / Tauri FS)
pub trait LocalSync {
    fn pick_directory(&self) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<DirectoryHandle, String>>>>;
    fn export_project(&self, handle: &DirectoryHandle, files: &[ProjectFile]) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), String>>>>;
    fn import_directory(&self, handle: &DirectoryHandle) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Vec<ProjectFile>, String>>>>;
}

/// Opaque handle to a directory (browser FileSystemDirectoryHandle or Tauri path)
#[derive(Clone, Debug)]
pub enum DirectoryHandle {
    Browser(wasm_bindgen::JsValue),
    Path(String),
}
