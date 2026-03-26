use super::traits::ProjectStorage;
use super::indexeddb::IndexedDbStorage;
use super::server_api::ServerApiStorage;
use serde::{Deserialize, Serialize};

/// Available storage backends
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum StorageBackend {
    IndexedDb,
    ServerApi,
}

impl StorageBackend {
    pub fn label(&self) -> &str {
        match self {
            StorageBackend::IndexedDb => "Local (Browser)",
            StorageBackend::ServerApi => "Server",
        }
    }
}

impl Default for StorageBackend {
    fn default() -> Self {
        StorageBackend::IndexedDb
    }
}

/// Load the selected backend from localStorage
pub fn load_backend_choice() -> StorageBackend {
    if let Some(window) = web_sys::window() {
        if let Ok(Some(storage)) = window.local_storage() {
            if let Ok(Some(val)) = storage.get_item("storage_backend") {
                return match val.as_str() {
                    "server_api" => StorageBackend::ServerApi,
                    _ => StorageBackend::IndexedDb,
                };
            }
        }
    }
    StorageBackend::default()
}

/// Save the selected backend to localStorage
pub fn save_backend_choice(backend: &StorageBackend) {
    if let Some(window) = web_sys::window() {
        if let Ok(Some(storage)) = window.local_storage() {
            let val = match backend {
                StorageBackend::IndexedDb => "indexeddb",
                StorageBackend::ServerApi => "server_api",
            };
            let _ = storage.set_item("storage_backend", val);
        }
    }
}

/// Create a storage instance based on the selected backend
pub fn create_storage() -> Box<dyn ProjectStorage + Send + Sync> {
    let backend = load_backend_choice();
    create_storage_for_backend(&backend)
}

/// Create a storage instance for a specific backend
pub fn create_storage_for_backend(backend: &StorageBackend) -> Box<dyn ProjectStorage + Send + Sync> {
    match backend {
        StorageBackend::IndexedDb => Box::new(IndexedDbStorage::new()),
        StorageBackend::ServerApi => Box::new(ServerApiStorage::new()),
    }
}
