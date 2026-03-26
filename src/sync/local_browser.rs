use super::traits::{LocalSync, DirectoryHandle};
use crate::models::{ProjectFile, FileContent, is_text_file};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use std::pin::Pin;
use std::future::Future;

/// Check if the File System Access API is available (Chromium-only)
pub fn is_fs_access_supported() -> bool {
    web_sys::window()
        .and_then(|w| {
            js_sys::Reflect::get(&w, &JsValue::from_str("showDirectoryPicker"))
                .ok()
                .filter(|v| v.is_function())
        })
        .is_some()
}

/// Browser-based local filesystem sync using File System Access API
pub struct BrowserLocalSync;

impl BrowserLocalSync {
    pub fn new() -> Self {
        Self
    }

    /// Call window.showDirectoryPicker() via js-sys
    async fn show_directory_picker() -> Result<JsValue, String> {
        let window = web_sys::window().ok_or("No window")?;
        let func = js_sys::Reflect::get(&window, &JsValue::from_str("showDirectoryPicker"))
            .map_err(|_| "showDirectoryPicker not available")?;
        let func = func.dyn_into::<js_sys::Function>()
            .map_err(|_| "showDirectoryPicker is not a function")?;
        let promise = func.call0(&window)
            .map_err(|e| format!("showDirectoryPicker failed: {:?}", e))?;
        let promise = promise.dyn_into::<js_sys::Promise>()
            .map_err(|_| "Not a promise")?;
        JsFuture::from(promise).await.map_err(|e| format!("Directory picker cancelled: {:?}", e))
    }

    /// Read all files from a FileSystemDirectoryHandle recursively
    fn read_directory_recursive<'a>(dir_handle: &'a JsValue, base_path: &'a str) -> Pin<Box<dyn Future<Output = Result<Vec<ProjectFile>, String>> + 'a>> {
        Box::pin(async move {
        let mut files = Vec::new();

        // Use entries() iterator
        let entries_fn = js_sys::Reflect::get(dir_handle, &JsValue::from_str("entries"))
            .map_err(|_| "No entries method")?;
        let entries_fn = entries_fn.dyn_into::<js_sys::Function>()
            .map_err(|_| "entries is not a function")?;
        let iterator = entries_fn.call0(dir_handle)
            .map_err(|e| format!("entries() failed: {:?}", e))?;

        loop {
            let next_fn = js_sys::Reflect::get(&iterator, &JsValue::from_str("next"))
                .map_err(|_| "No next method")?;
            let next_fn = next_fn.dyn_into::<js_sys::Function>()
                .map_err(|_| "next is not a function")?;
            let result = next_fn.call0(&iterator)
                .map_err(|e| format!("next() failed: {:?}", e))?;

            // result is a Promise
            let result = JsFuture::from(result.dyn_into::<js_sys::Promise>().map_err(|_| "Not a promise")?)
                .await
                .map_err(|e| format!("Iterator failed: {:?}", e))?;

            let done = js_sys::Reflect::get(&result, &JsValue::from_str("done"))
                .ok()
                .and_then(|v| v.as_bool())
                .unwrap_or(true);

            if done {
                break;
            }

            let value = js_sys::Reflect::get(&result, &JsValue::from_str("value"))
                .map_err(|_| "No value")?;

            // value is [name, handle]
            let arr = value.dyn_into::<js_sys::Array>().map_err(|_| "Not an array")?;
            let name = arr.get(0).as_string().unwrap_or_default();
            let handle = arr.get(1);

            let kind = js_sys::Reflect::get(&handle, &JsValue::from_str("kind"))
                .ok()
                .and_then(|v| v.as_string())
                .unwrap_or_default();

            let path = if base_path.is_empty() {
                name.clone()
            } else {
                format!("{}/{}", base_path, name)
            };

            if kind == "file" {
                // Read file content
                let get_file_fn = js_sys::Reflect::get(&handle, &JsValue::from_str("getFile"))
                    .map_err(|_| "No getFile")?
                    .dyn_into::<js_sys::Function>()
                    .map_err(|_| "getFile not a function")?;
                let file_promise = get_file_fn.call0(&handle)
                    .map_err(|e| format!("getFile failed: {:?}", e))?
                    .dyn_into::<js_sys::Promise>()
                    .map_err(|_| "Not a promise")?;
                let file = JsFuture::from(file_promise).await
                    .map_err(|e| format!("getFile await failed: {:?}", e))?;

                if is_text_file(&name) {
                    // Read as text
                    let text_fn = js_sys::Reflect::get(&file, &JsValue::from_str("text"))
                        .map_err(|_| "No text method")?
                        .dyn_into::<js_sys::Function>()
                        .map_err(|_| "text not a function")?;
                    let text_promise = text_fn.call0(&file)
                        .map_err(|e| format!("text() failed: {:?}", e))?
                        .dyn_into::<js_sys::Promise>()
                        .map_err(|_| "Not a promise")?;
                    let text = JsFuture::from(text_promise).await
                        .map_err(|e| format!("text await failed: {:?}", e))?;
                    let text_str = text.as_string().unwrap_or_default();
                    files.push(ProjectFile {
                        path,
                        content: FileContent::Text(text_str),
                    });
                } else {
                    // Read as binary
                    let buf_fn = js_sys::Reflect::get(&file, &JsValue::from_str("arrayBuffer"))
                        .map_err(|_| "No arrayBuffer")?
                        .dyn_into::<js_sys::Function>()
                        .map_err(|_| "arrayBuffer not a function")?;
                    let buf_promise = buf_fn.call0(&file)
                        .map_err(|e| format!("arrayBuffer() failed: {:?}", e))?
                        .dyn_into::<js_sys::Promise>()
                        .map_err(|_| "Not a promise")?;
                    let buf = JsFuture::from(buf_promise).await
                        .map_err(|e| format!("arrayBuffer await failed: {:?}", e))?;
                    let uint8 = js_sys::Uint8Array::new(&buf);
                    files.push(ProjectFile {
                        path,
                        content: FileContent::Binary(uint8.to_vec()),
                    });
                }
            } else if kind == "directory" {
                // Skip hidden directories
                if !name.starts_with('.') {
                    let sub_files = Self::read_directory_recursive(&handle, &path).await?;
                    files.extend(sub_files);
                }
            }
        }

        Ok(files)
        }) // close Box::pin(async move {
    }

    /// Write a file to a FileSystemDirectoryHandle
    async fn write_to_directory(dir_handle: &JsValue, path: &str, data: &[u8]) -> Result<(), String> {
        let parts: Vec<&str> = path.split('/').collect();

        // Navigate/create subdirectories
        let mut current_dir = dir_handle.clone();
        for dir_name in &parts[..parts.len() - 1] {
            let get_dir_fn = js_sys::Reflect::get(&current_dir, &JsValue::from_str("getDirectoryHandle"))
                .map_err(|_| "No getDirectoryHandle")?
                .dyn_into::<js_sys::Function>()
                .map_err(|_| "Not a function")?;

            let options = js_sys::Object::new();
            js_sys::Reflect::set(&options, &JsValue::from_str("create"), &JsValue::TRUE).ok();

            let promise = get_dir_fn.call2(&current_dir, &JsValue::from_str(dir_name), &options)
                .map_err(|e| format!("getDirectoryHandle failed: {:?}", e))?
                .dyn_into::<js_sys::Promise>()
                .map_err(|_| "Not a promise")?;

            current_dir = JsFuture::from(promise).await
                .map_err(|e| format!("getDirectoryHandle await failed: {:?}", e))?;
        }

        // Create/get file handle
        let file_name = parts.last().ok_or("Empty path")?;
        let get_file_fn = js_sys::Reflect::get(&current_dir, &JsValue::from_str("getFileHandle"))
            .map_err(|_| "No getFileHandle")?
            .dyn_into::<js_sys::Function>()
            .map_err(|_| "Not a function")?;

        let options = js_sys::Object::new();
        js_sys::Reflect::set(&options, &JsValue::from_str("create"), &JsValue::TRUE).ok();

        let file_handle_promise = get_file_fn.call2(&current_dir, &JsValue::from_str(file_name), &options)
            .map_err(|e| format!("getFileHandle failed: {:?}", e))?
            .dyn_into::<js_sys::Promise>()
            .map_err(|_| "Not a promise")?;

        let file_handle = JsFuture::from(file_handle_promise).await
            .map_err(|e| format!("getFileHandle await failed: {:?}", e))?;

        // Create writable stream
        let create_writable_fn = js_sys::Reflect::get(&file_handle, &JsValue::from_str("createWritable"))
            .map_err(|_| "No createWritable")?
            .dyn_into::<js_sys::Function>()
            .map_err(|_| "Not a function")?;

        let writable_promise = create_writable_fn.call0(&file_handle)
            .map_err(|e| format!("createWritable failed: {:?}", e))?
            .dyn_into::<js_sys::Promise>()
            .map_err(|_| "Not a promise")?;

        let writable = JsFuture::from(writable_promise).await
            .map_err(|e| format!("createWritable await failed: {:?}", e))?;

        // Write data
        let uint8 = js_sys::Uint8Array::from(data);
        let write_fn = js_sys::Reflect::get(&writable, &JsValue::from_str("write"))
            .map_err(|_| "No write")?
            .dyn_into::<js_sys::Function>()
            .map_err(|_| "Not a function")?;

        let write_promise = write_fn.call1(&writable, &uint8)
            .map_err(|e| format!("write failed: {:?}", e))?
            .dyn_into::<js_sys::Promise>()
            .map_err(|_| "Not a promise")?;

        JsFuture::from(write_promise).await
            .map_err(|e| format!("write await failed: {:?}", e))?;

        // Close stream
        let close_fn = js_sys::Reflect::get(&writable, &JsValue::from_str("close"))
            .map_err(|_| "No close")?
            .dyn_into::<js_sys::Function>()
            .map_err(|_| "Not a function")?;

        let close_promise = close_fn.call0(&writable)
            .map_err(|e| format!("close failed: {:?}", e))?
            .dyn_into::<js_sys::Promise>()
            .map_err(|_| "Not a promise")?;

        JsFuture::from(close_promise).await
            .map_err(|e| format!("close await failed: {:?}", e))?;

        Ok(())
    }
}

impl LocalSync for BrowserLocalSync {
    fn pick_directory(&self) -> Pin<Box<dyn Future<Output = Result<DirectoryHandle, String>>>> {
        Box::pin(async move {
            let handle = Self::show_directory_picker().await?;
            Ok(DirectoryHandle::Browser(handle))
        })
    }

    fn export_project(&self, handle: &DirectoryHandle, files: &[ProjectFile]) -> Pin<Box<dyn Future<Output = Result<(), String>>>> {
        let handle = match handle {
            DirectoryHandle::Browser(h) => h.clone(),
            _ => return Box::pin(async { Err("Expected browser directory handle".to_string()) }),
        };
        let files: Vec<ProjectFile> = files.to_vec();

        Box::pin(async move {
            for file in &files {
                let data = file.content.as_bytes();
                Self::write_to_directory(&handle, &file.path, &data).await?;
                log::info!("Exported: {}", file.path);
            }
            log::info!("Export complete: {} files", files.len());
            Ok(())
        })
    }

    fn import_directory(&self, handle: &DirectoryHandle) -> Pin<Box<dyn Future<Output = Result<Vec<ProjectFile>, String>>>> {
        let handle = match handle {
            DirectoryHandle::Browser(h) => h.clone(),
            _ => return Box::pin(async { Err("Expected browser directory handle".to_string()) }),
        };

        Box::pin(async move {
            let files = Self::read_directory_recursive(&handle, "").await?;
            log::info!("Imported {} files from directory", files.len());
            Ok(files)
        })
    }
}
