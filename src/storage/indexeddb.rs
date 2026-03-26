use crate::models::{Project, ProjectMetadata, FileContent};
use base64::Engine as _;
use super::traits::ProjectStorage;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use web_sys::{IdbDatabase, IdbTransactionMode};
use std::pin::Pin;
use std::future::Future;

const DB_NAME: &str = "typst_studio_v2";
const DB_VERSION: u32 = 1;
const PROJECTS_STORE: &str = "projects";
const FILES_STORE: &str = "files";

pub struct IndexedDbStorage;

impl IndexedDbStorage {
    pub fn new() -> Self {
        Self
    }

    async fn open_db() -> Result<IdbDatabase, String> {
        let window = web_sys::window().ok_or("No window")?;
        let idb_factory = window
            .indexed_db()
            .map_err(|_| "IndexedDB not supported")?
            .ok_or("IndexedDB not available")?;

        let open_request = idb_factory
            .open_with_u32(DB_NAME, DB_VERSION)
            .map_err(|e| format!("Failed to open DB: {:?}", e))?;

        let onupgradeneeded = Closure::wrap(Box::new(move |event: web_sys::IdbVersionChangeEvent| {
            if let Some(target) = event.target() {
                if let Ok(request) = target.dyn_into::<web_sys::IdbOpenDbRequest>() {
                    if let Ok(result) = request.result() {
                        if let Ok(db) = result.dyn_into::<IdbDatabase>() {
                            let store_names = db.object_store_names();
                            if !store_names.contains(PROJECTS_STORE) {
                                let _ = db.create_object_store(PROJECTS_STORE);
                            }
                            if !store_names.contains(FILES_STORE) {
                                let _ = db.create_object_store(FILES_STORE);
                            }
                        }
                    }
                }
            }
        }) as Box<dyn FnMut(_)>);

        open_request.set_onupgradeneeded(Some(onupgradeneeded.as_ref().unchecked_ref()));
        onupgradeneeded.forget();

        let promise = js_sys::Promise::new(&mut |resolve, reject| {
            let req = open_request.clone();
            let onsuccess = Closure::wrap(Box::new(move |_: web_sys::Event| {
                if let Ok(result) = req.result() {
                    let _ = resolve.call1(&JsValue::NULL, &result);
                }
            }) as Box<dyn FnMut(_)>);

            let onerror = Closure::wrap(Box::new(move |_: web_sys::Event| {
                let _ = reject.call1(&JsValue::NULL, &JsValue::from_str("Failed to open DB"));
            }) as Box<dyn FnMut(_)>);

            open_request.set_onsuccess(Some(onsuccess.as_ref().unchecked_ref()));
            open_request.set_onerror(Some(onerror.as_ref().unchecked_ref()));
            onsuccess.forget();
            onerror.forget();
        });

        let result = JsFuture::from(promise).await.map_err(|e| format!("{:?}", e))?;
        result.dyn_into::<IdbDatabase>().map_err(|_| "Failed to cast to IdbDatabase".to_string())
    }

    fn request_to_future(request: &web_sys::IdbRequest) -> Pin<Box<dyn Future<Output = Result<JsValue, String>>>> {
        let promise = {
            let req = request.clone();
            js_sys::Promise::new(&mut |resolve, reject| {
                let req_s = req.clone();
                let onsuccess = Closure::wrap(Box::new(move |_: web_sys::Event| {
                    if let Ok(result) = req_s.result() {
                        let _ = resolve.call1(&JsValue::NULL, &result);
                    }
                }) as Box<dyn FnMut(_)>);
                let onerror = Closure::wrap(Box::new(move |_: web_sys::Event| {
                    let _ = reject.call1(&JsValue::NULL, &JsValue::from_str("IDB request failed"));
                }) as Box<dyn FnMut(_)>);
                req.set_onsuccess(Some(onsuccess.as_ref().unchecked_ref()));
                req.set_onerror(Some(onerror.as_ref().unchecked_ref()));
                onsuccess.forget();
                onerror.forget();
            })
        };
        Box::pin(async move {
            JsFuture::from(promise).await.map_err(|e| format!("{:?}", e))
        })
    }

    /// Composite key for files: "project_id/path"
    fn file_key(project_id: &str, path: &str) -> String {
        format!("{}/{}", project_id, path)
    }
}

impl ProjectStorage for IndexedDbStorage {
    fn list_projects(&self) -> Pin<Box<dyn Future<Output = Result<Vec<ProjectMetadata>, String>>>> {
        Box::pin(async move {
            // Get all project data in a SINGLE request (no loop + await which kills transactions)
            let db = Self::open_db().await?;
            let tx = db.transaction_with_str(PROJECTS_STORE).map_err(|e| format!("{:?}", e))?;
            let store = tx.object_store(PROJECTS_STORE).map_err(|e| format!("{:?}", e))?;

            // get_all() returns all values in one shot — no transaction issues
            let all_req = store.get_all().map_err(|e| format!("{:?}", e))?;
            let all_keys_req = store.get_all_keys().map_err(|e| format!("{:?}", e))?;
            let all_vals = Self::request_to_future(&all_req).await?;
            let all_keys = Self::request_to_future(&all_keys_req).await?;

            let vals_array: js_sys::Array = all_vals.dyn_into().map_err(|_| "Not an array")?;
            let keys_array: js_sys::Array = all_keys.dyn_into().map_err(|_| "Not an array")?;

            // Get all file keys in one shot for counting
            let file_keys = {
                let db2 = Self::open_db().await?;
                let ftx = db2.transaction_with_str(FILES_STORE).map_err(|e| format!("{:?}", e))?;
                let fstore = ftx.object_store(FILES_STORE).map_err(|e| format!("{:?}", e))?;
                let fk_req = fstore.get_all_keys().map_err(|e| format!("{:?}", e))?;
                let fk = Self::request_to_future(&fk_req).await?;
                let fk_arr: js_sys::Array = fk.dyn_into().map_err(|_| "Not an array")?;
                let mut keys = Vec::new();
                for i in 0..fk_arr.length() {
                    if let Some(k) = fk_arr.get(i).as_string() {
                        keys.push(k);
                    }
                }
                keys
            };

            let mut projects = Vec::new();
            for i in 0..vals_array.length() {
                let val = vals_array.get(i);
                let key = keys_array.get(i);
                if let (Some(json_str), Some(id)) = (val.as_string(), key.as_string()) {
                    if let Ok(project) = serde_json::from_str::<Project>(&json_str) {
                        let prefix = format!("{}/", id);
                        let file_count = file_keys.iter().filter(|k| k.starts_with(&prefix)).count();
                        let mut meta = ProjectMetadata::from(&project);
                        meta.file_count = file_count;
                        projects.push(meta);
                    }
                }
            }

            projects.sort_by(|a, b| b.updated_at.partial_cmp(&a.updated_at).unwrap_or(std::cmp::Ordering::Equal));
            Ok(projects)
        })
    }

    fn get_project(&self, id: &str) -> Pin<Box<dyn Future<Output = Result<Project, String>>>> {
        let id = id.to_string();
        Box::pin(async move {
            let db = Self::open_db().await?;
            let tx = db.transaction_with_str(PROJECTS_STORE).map_err(|e| format!("{:?}", e))?;
            let store = tx.object_store(PROJECTS_STORE).map_err(|e| format!("{:?}", e))?;

            let request = store.get(&JsValue::from_str(&id)).map_err(|e| format!("{:?}", e))?;
            let val = Self::request_to_future(&request).await?;

            if val.is_undefined() || val.is_null() {
                return Err(format!("Project not found: {}", id));
            }

            let json_str = val.as_string().ok_or("Project data is not a string")?;
            serde_json::from_str(&json_str).map_err(|e| format!("Failed to parse project: {}", e))
        })
    }

    fn create_project(&self, name: &str) -> Pin<Box<dyn Future<Output = Result<Project, String>>>> {
        let name = name.to_string();
        Box::pin(async move {
            let project = Project::new(name);

            // Store project metadata
            {
                let db = Self::open_db().await?;
                let tx = db.transaction_with_str_and_mode(PROJECTS_STORE, IdbTransactionMode::Readwrite)
                    .map_err(|e| format!("{:?}", e))?;
                let store = tx.object_store(PROJECTS_STORE).map_err(|e| format!("{:?}", e))?;
                let json = serde_json::to_string(&project).map_err(|e| format!("{}", e))?;
                let request = store.put_with_key(&JsValue::from_str(&json), &JsValue::from_str(&project.id))
                    .map_err(|e| format!("{:?}", e))?;
                Self::request_to_future(&request).await?;
            }

            // Create default files using write_file (separate transactions, safe with async)
            let default_content = include_str!("../../examples/example.typ");
            let default_bib = include_str!("../../examples/refs.yml");

            // main.typ
            {
                let db = Self::open_db().await?;
                let key = Self::file_key(&project.id, "main.typ");
                let data = serde_json::json!({ "type": "text", "content": default_content }).to_string();
                let tx = db.transaction_with_str_and_mode(FILES_STORE, IdbTransactionMode::Readwrite)
                    .map_err(|e| format!("{:?}", e))?;
                let store = tx.object_store(FILES_STORE).map_err(|e| format!("{:?}", e))?;
                let req = store.put_with_key(&JsValue::from_str(&data), &JsValue::from_str(&key))
                    .map_err(|e| format!("{:?}", e))?;
                Self::request_to_future(&req).await?;
            }

            // refs.yml
            {
                let db = Self::open_db().await?;
                let key = Self::file_key(&project.id, "refs.yml");
                let data = serde_json::json!({ "type": "text", "content": default_bib }).to_string();
                let tx = db.transaction_with_str_and_mode(FILES_STORE, IdbTransactionMode::Readwrite)
                    .map_err(|e| format!("{:?}", e))?;
                let store = tx.object_store(FILES_STORE).map_err(|e| format!("{:?}", e))?;
                let req = store.put_with_key(&JsValue::from_str(&data), &JsValue::from_str(&key))
                    .map_err(|e| format!("{:?}", e))?;
                Self::request_to_future(&req).await?;
            }

            log::info!("Created project: {} ({})", project.name, project.id);
            Ok(project)
        })
    }

    fn delete_project(&self, id: &str) -> Pin<Box<dyn Future<Output = Result<(), String>>>> {
        let id = id.to_string();
        Box::pin(async move {
            let db = Self::open_db().await?;

            // Delete project metadata
            let tx = db.transaction_with_str_and_mode(PROJECTS_STORE, IdbTransactionMode::Readwrite)
                .map_err(|e| format!("{:?}", e))?;
            let store = tx.object_store(PROJECTS_STORE).map_err(|e| format!("{:?}", e))?;
            let request = store.delete(&JsValue::from_str(&id)).map_err(|e| format!("{:?}", e))?;
            Self::request_to_future(&request).await?;

            // Delete all files for this project
            let ftx = db.transaction_with_str_and_mode(FILES_STORE, IdbTransactionMode::Readwrite)
                .map_err(|e| format!("{:?}", e))?;
            let fstore = ftx.object_store(FILES_STORE).map_err(|e| format!("{:?}", e))?;
            let keys_req = fstore.get_all_keys().map_err(|e| format!("{:?}", e))?;
            let keys = Self::request_to_future(&keys_req).await?;
            let keys_array: js_sys::Array = keys.dyn_into().map_err(|_| "Not an array")?;

            let prefix = format!("{}/", id);
            for i in 0..keys_array.length() {
                if let Some(key_str) = keys_array.get(i).as_string() {
                    if key_str.starts_with(&prefix) {
                        let del_req = fstore.delete(&JsValue::from_str(&key_str)).map_err(|e| format!("{:?}", e))?;
                        Self::request_to_future(&del_req).await?;
                    }
                }
            }

            log::info!("Deleted project: {}", id);
            Ok(())
        })
    }

    fn update_project(&self, project: &Project) -> Pin<Box<dyn Future<Output = Result<(), String>>>> {
        let project = project.clone();
        Box::pin(async move {
            let db = Self::open_db().await?;
            let tx = db.transaction_with_str_and_mode(PROJECTS_STORE, IdbTransactionMode::Readwrite)
                .map_err(|e| format!("{:?}", e))?;
            let store = tx.object_store(PROJECTS_STORE).map_err(|e| format!("{:?}", e))?;
            let json = serde_json::to_string(&project).map_err(|e| format!("{}", e))?;
            let request = store.put_with_key(&JsValue::from_str(&json), &JsValue::from_str(&project.id))
                .map_err(|e| format!("{:?}", e))?;
            Self::request_to_future(&request).await?;
            Ok(())
        })
    }

    fn list_files(&self, project_id: &str) -> Pin<Box<dyn Future<Output = Result<Vec<String>, String>>>> {
        let project_id = project_id.to_string();
        Box::pin(async move {
            let db = Self::open_db().await?;
            let tx = db.transaction_with_str(FILES_STORE).map_err(|e| format!("{:?}", e))?;
            let store = tx.object_store(FILES_STORE).map_err(|e| format!("{:?}", e))?;

            let keys_req = store.get_all_keys().map_err(|e| format!("{:?}", e))?;
            let keys = Self::request_to_future(&keys_req).await?;
            let keys_array: js_sys::Array = keys.dyn_into().map_err(|_| "Not an array")?;

            let prefix = format!("{}/", project_id);
            let mut files = Vec::new();
            for i in 0..keys_array.length() {
                if let Some(key_str) = keys_array.get(i).as_string() {
                    if let Some(path) = key_str.strip_prefix(&prefix) {
                        files.push(path.to_string());
                    }
                }
            }

            files.sort();
            Ok(files)
        })
    }

    fn read_file(&self, project_id: &str, path: &str) -> Pin<Box<dyn Future<Output = Result<FileContent, String>>>> {
        let key = Self::file_key(project_id, path);
        let path = path.to_string();
        Box::pin(async move {
            let db = Self::open_db().await?;
            let tx = db.transaction_with_str(FILES_STORE).map_err(|e| format!("{:?}", e))?;
            let store = tx.object_store(FILES_STORE).map_err(|e| format!("{:?}", e))?;

            let request = store.get(&JsValue::from_str(&key)).map_err(|e| format!("{:?}", e))?;
            let val = Self::request_to_future(&request).await?;

            if val.is_undefined() || val.is_null() {
                return Err(format!("File not found: {}", path));
            }

            let json_str = val.as_string().ok_or("File data is not a string")?;
            let parsed: serde_json::Value = serde_json::from_str(&json_str)
                .map_err(|e| format!("Parse error: {}", e))?;

            match parsed.get("type").and_then(|t| t.as_str()) {
                Some("text") => {
                    let content = parsed.get("content").and_then(|c| c.as_str()).unwrap_or("").to_string();
                    Ok(FileContent::Text(content))
                }
                Some("binary") => {
                    let b64 = parsed.get("content").and_then(|c| c.as_str()).unwrap_or("");
                    let bytes = base64::engine::general_purpose::STANDARD.decode(b64)
                        .map_err(|e| format!("Base64 decode error: {}", e))?;
                    Ok(FileContent::Binary(bytes))
                }
                _ => Err(format!("Unknown file type for: {}", path)),
            }
        })
    }

    fn write_file(&self, project_id: &str, path: &str, content: &FileContent) -> Pin<Box<dyn Future<Output = Result<(), String>>>> {
        let key = Self::file_key(project_id, path);
        let file_data = match content {
            FileContent::Text(text) => {
                serde_json::json!({
                    "type": "text",
                    "content": text
                }).to_string()
            }
            FileContent::Binary(bytes) => {
                let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
                serde_json::json!({
                    "type": "binary",
                    "content": b64
                }).to_string()
            }
        };

        Box::pin(async move {
            let db = Self::open_db().await?;
            let tx = db.transaction_with_str_and_mode(FILES_STORE, IdbTransactionMode::Readwrite)
                .map_err(|e| format!("{:?}", e))?;
            let store = tx.object_store(FILES_STORE).map_err(|e| format!("{:?}", e))?;
            let request = store.put_with_key(&JsValue::from_str(&file_data), &JsValue::from_str(&key))
                .map_err(|e| format!("{:?}", e))?;
            Self::request_to_future(&request).await?;
            Ok(())
        })
    }

    fn delete_file(&self, project_id: &str, path: &str) -> Pin<Box<dyn Future<Output = Result<(), String>>>> {
        let key = Self::file_key(project_id, path);
        Box::pin(async move {
            let db = Self::open_db().await?;
            let tx = db.transaction_with_str_and_mode(FILES_STORE, IdbTransactionMode::Readwrite)
                .map_err(|e| format!("{:?}", e))?;
            let store = tx.object_store(FILES_STORE).map_err(|e| format!("{:?}", e))?;
            let request = store.delete(&JsValue::from_str(&key)).map_err(|e| format!("{:?}", e))?;
            Self::request_to_future(&request).await?;
            Ok(())
        })
    }

    fn rename_file(&self, project_id: &str, old_path: &str, new_path: &str) -> Pin<Box<dyn Future<Output = Result<(), String>>>> {
        let old_key = Self::file_key(project_id, old_path);
        let new_key = Self::file_key(project_id, new_path);
        Box::pin(async move {
            // Read old file
            let val = {
                let db = Self::open_db().await?;
                let tx = db.transaction_with_str(FILES_STORE).map_err(|e| format!("{:?}", e))?;
                let store = tx.object_store(FILES_STORE).map_err(|e| format!("{:?}", e))?;
                let request = store.get(&JsValue::from_str(&old_key)).map_err(|e| format!("{:?}", e))?;
                Self::request_to_future(&request).await?
            };

            if val.is_undefined() || val.is_null() {
                return Err("Source file not found".to_string());
            }

            // Write to new key
            {
                let db = Self::open_db().await?;
                let tx = db.transaction_with_str_and_mode(FILES_STORE, IdbTransactionMode::Readwrite)
                    .map_err(|e| format!("{:?}", e))?;
                let store = tx.object_store(FILES_STORE).map_err(|e| format!("{:?}", e))?;
                let req = store.put_with_key(&val, &JsValue::from_str(&new_key))
                    .map_err(|e| format!("{:?}", e))?;
                Self::request_to_future(&req).await?;
            }

            // Delete old key
            {
                let db = Self::open_db().await?;
                let tx = db.transaction_with_str_and_mode(FILES_STORE, IdbTransactionMode::Readwrite)
                    .map_err(|e| format!("{:?}", e))?;
                let store = tx.object_store(FILES_STORE).map_err(|e| format!("{:?}", e))?;
                let req = store.delete(&JsValue::from_str(&old_key))
                    .map_err(|e| format!("{:?}", e))?;
                Self::request_to_future(&req).await?;
            }

            Ok(())
        })
    }
}

impl IndexedDbStorage {
    async fn count_project_files(db: &IdbDatabase, project_id: &str) -> Result<usize, String> {
        let tx = db.transaction_with_str(FILES_STORE).map_err(|e| format!("{:?}", e))?;
        let store = tx.object_store(FILES_STORE).map_err(|e| format!("{:?}", e))?;
        let keys_req = store.get_all_keys().map_err(|e| format!("{:?}", e))?;
        let keys = Self::request_to_future(&keys_req).await?;
        let keys_array: js_sys::Array = keys.dyn_into().map_err(|_| "Not an array")?;

        let prefix = format!("{}/", project_id);
        let mut count = 0;
        for i in 0..keys_array.length() {
            if let Some(key_str) = keys_array.get(i).as_string() {
                if key_str.starts_with(&prefix) {
                    count += 1;
                }
            }
        }
        Ok(count)
    }
}
