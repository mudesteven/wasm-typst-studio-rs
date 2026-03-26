use crate::models::{Project, ProjectMetadata, FileContent, is_text_file};
use base64::Engine as _;
use super::traits::ProjectStorage;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use std::pin::Pin;
use std::future::Future;

/// Server API storage backend.
/// Talks to a simple file server that stores raw project files in folders.
pub struct ServerApiStorage {
    base_url: String,
    auth_token: Option<String>,
}

impl ServerApiStorage {
    pub fn new() -> Self {
        let base_url = Self::load_setting("server_api_url")
            .unwrap_or_else(|| "http://localhost:3001/api".to_string());
        let auth_token = Self::load_setting("server_api_token");
        Self { base_url, auth_token }
    }

    fn load_setting(key: &str) -> Option<String> {
        web_sys::window()
            .and_then(|w| w.local_storage().ok().flatten())
            .and_then(|s| s.get_item(key).ok().flatten())
            .filter(|v| !v.is_empty())
    }

    async fn fetch(&self, method: &str, path: &str, body: Option<&str>) -> Result<web_sys::Response, String> {
        let url = format!("{}{}", self.base_url, path);

        let opts = web_sys::RequestInit::new();
        opts.set_method(method);

        if let Some(body_str) = body {
            opts.set_body(&JsValue::from_str(body_str));
        }

        let request = web_sys::Request::new_with_str_and_init(&url, &opts)
            .map_err(|e| format!("Request creation failed: {:?}", e))?;

        request.headers().set("Content-Type", "application/json").ok();
        if let Some(token) = &self.auth_token {
            request.headers().set("Authorization", &format!("Bearer {}", token)).ok();
        }

        let window = web_sys::window().ok_or("No window")?;
        let resp_value = JsFuture::from(window.fetch_with_request(&request))
            .await
            .map_err(|e| format!("Fetch failed: {:?}", e))?;

        let resp: web_sys::Response = resp_value.dyn_into().map_err(|_| "Not a Response")?;

        if !resp.ok() {
            let status = resp.status();
            let text = JsFuture::from(resp.text().map_err(|_| "No text")?)
                .await
                .ok()
                .and_then(|v| v.as_string())
                .unwrap_or_default();
            return Err(format!("Server error {}: {}", status, text));
        }

        Ok(resp)
    }

    async fn fetch_json(&self, method: &str, path: &str, body: Option<&str>) -> Result<JsValue, String> {
        let resp = self.fetch(method, path, body).await?;
        JsFuture::from(resp.json().map_err(|_| "No JSON")?)
            .await
            .map_err(|e| format!("JSON parse failed: {:?}", e))
    }

    async fn fetch_text(&self, method: &str, path: &str, body: Option<&str>) -> Result<String, String> {
        let resp = self.fetch(method, path, body).await?;
        let text = JsFuture::from(resp.text().map_err(|_| "No text")?)
            .await
            .map_err(|e| format!("Text read failed: {:?}", e))?;
        text.as_string().ok_or("Response is not text".to_string())
    }

    async fn fetch_bytes(&self, method: &str, path: &str) -> Result<Vec<u8>, String> {
        let resp = self.fetch(method, path, None).await?;
        let buf = JsFuture::from(resp.array_buffer().map_err(|_| "No buffer")?)
            .await
            .map_err(|e| format!("Buffer read failed: {:?}", e))?;
        let uint8 = js_sys::Uint8Array::new(&buf);
        Ok(uint8.to_vec())
    }
}

impl ProjectStorage for ServerApiStorage {
    fn list_projects(&self) -> Pin<Box<dyn Future<Output = Result<Vec<ProjectMetadata>, String>>>> {
        let base_url = self.base_url.clone();
        let auth_token = self.auth_token.clone();
        Box::pin(async move {
            let storage = ServerApiStorage { base_url, auth_token };
            let json = storage.fetch_json("GET", "/projects", None).await?;
            let json_str = js_sys::JSON::stringify(&json)
                .map_err(|_| "Stringify failed")?
                .as_string()
                .unwrap_or_default();
            serde_json::from_str(&json_str).map_err(|e| format!("Parse error: {}", e))
        })
    }

    fn get_project(&self, id: &str) -> Pin<Box<dyn Future<Output = Result<Project, String>>>> {
        let base_url = self.base_url.clone();
        let auth_token = self.auth_token.clone();
        let id = id.to_string();
        Box::pin(async move {
            let storage = ServerApiStorage { base_url, auth_token };
            let path = format!("/projects/{}", id);
            let json = storage.fetch_json("GET", &path, None).await?;
            let json_str = js_sys::JSON::stringify(&json)
                .map_err(|_| "Stringify failed")?
                .as_string()
                .unwrap_or_default();
            serde_json::from_str(&json_str).map_err(|e| format!("Parse error: {}", e))
        })
    }

    fn create_project(&self, name: &str) -> Pin<Box<dyn Future<Output = Result<Project, String>>>> {
        let base_url = self.base_url.clone();
        let auth_token = self.auth_token.clone();
        let name = name.to_string();
        Box::pin(async move {
            let storage = ServerApiStorage { base_url, auth_token };
            let body = serde_json::json!({"name": name}).to_string();
            let json = storage.fetch_json("POST", "/projects", Some(&body)).await?;
            let json_str = js_sys::JSON::stringify(&json)
                .map_err(|_| "Stringify failed")?
                .as_string()
                .unwrap_or_default();
            serde_json::from_str(&json_str).map_err(|e| format!("Parse error: {}", e))
        })
    }

    fn delete_project(&self, id: &str) -> Pin<Box<dyn Future<Output = Result<(), String>>>> {
        let base_url = self.base_url.clone();
        let auth_token = self.auth_token.clone();
        let id = id.to_string();
        Box::pin(async move {
            let storage = ServerApiStorage { base_url, auth_token };
            let path = format!("/projects/{}", id);
            storage.fetch("DELETE", &path, None).await?;
            Ok(())
        })
    }

    fn update_project(&self, project: &Project) -> Pin<Box<dyn Future<Output = Result<(), String>>>> {
        let base_url = self.base_url.clone();
        let auth_token = self.auth_token.clone();
        let project = project.clone();
        Box::pin(async move {
            let storage = ServerApiStorage { base_url, auth_token };
            let path = format!("/projects/{}", project.id);
            let body = serde_json::to_string(&project).map_err(|e| format!("{}", e))?;
            storage.fetch("PUT", &path, Some(&body)).await?;
            Ok(())
        })
    }

    fn list_files(&self, project_id: &str) -> Pin<Box<dyn Future<Output = Result<Vec<String>, String>>>> {
        let base_url = self.base_url.clone();
        let auth_token = self.auth_token.clone();
        let project_id = project_id.to_string();
        Box::pin(async move {
            let storage = ServerApiStorage { base_url, auth_token };
            let path = format!("/projects/{}/files", project_id);
            let json = storage.fetch_json("GET", &path, None).await?;
            let json_str = js_sys::JSON::stringify(&json)
                .map_err(|_| "Stringify failed")?
                .as_string()
                .unwrap_or_default();
            serde_json::from_str(&json_str).map_err(|e| format!("Parse error: {}", e))
        })
    }

    fn read_file(&self, project_id: &str, path: &str) -> Pin<Box<dyn Future<Output = Result<FileContent, String>>>> {
        let base_url = self.base_url.clone();
        let auth_token = self.auth_token.clone();
        let project_id = project_id.to_string();
        let file_path = path.to_string();
        Box::pin(async move {
            let storage = ServerApiStorage { base_url, auth_token };
            let url_path = format!("/projects/{}/files/{}", project_id, file_path);
            if is_text_file(&file_path) {
                let text = storage.fetch_text("GET", &url_path, None).await?;
                Ok(FileContent::Text(text))
            } else {
                let bytes = storage.fetch_bytes("GET", &url_path).await?;
                Ok(FileContent::Binary(bytes))
            }
        })
    }

    fn write_file(&self, project_id: &str, path: &str, content: &FileContent) -> Pin<Box<dyn Future<Output = Result<(), String>>>> {
        let base_url = self.base_url.clone();
        let auth_token = self.auth_token.clone();
        let project_id = project_id.to_string();
        let file_path = path.to_string();
        let body = match content {
            FileContent::Text(text) => {
                serde_json::json!({"type": "text", "content": text}).to_string()
            }
            FileContent::Binary(bytes) => {
                let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
                serde_json::json!({"type": "binary", "content": b64}).to_string()
            }
        };
        Box::pin(async move {
            let storage = ServerApiStorage { base_url, auth_token };
            let url_path = format!("/projects/{}/files/{}", project_id, file_path);
            storage.fetch("PUT", &url_path, Some(&body)).await?;
            Ok(())
        })
    }

    fn delete_file(&self, project_id: &str, path: &str) -> Pin<Box<dyn Future<Output = Result<(), String>>>> {
        let base_url = self.base_url.clone();
        let auth_token = self.auth_token.clone();
        let project_id = project_id.to_string();
        let file_path = path.to_string();
        Box::pin(async move {
            let storage = ServerApiStorage { base_url, auth_token };
            let url_path = format!("/projects/{}/files/{}", project_id, file_path);
            storage.fetch("DELETE", &url_path, None).await?;
            Ok(())
        })
    }

    fn rename_file(&self, project_id: &str, old_path: &str, new_path: &str) -> Pin<Box<dyn Future<Output = Result<(), String>>>> {
        let base_url = self.base_url.clone();
        let auth_token = self.auth_token.clone();
        let project_id = project_id.to_string();
        let old_path = old_path.to_string();
        let new_path = new_path.to_string();
        Box::pin(async move {
            let storage = ServerApiStorage { base_url, auth_token };
            let url_path = format!("/projects/{}/files/{}", project_id, old_path);
            let body = serde_json::json!({"new_path": new_path}).to_string();
            storage.fetch("PATCH", &url_path, Some(&body)).await?;
            Ok(())
        })
    }
}
