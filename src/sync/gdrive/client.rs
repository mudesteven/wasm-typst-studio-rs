use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use serde::{Deserialize, Serialize};

const DRIVE_API: &str = "https://www.googleapis.com/drive/v3";
const UPLOAD_API: &str = "https://www.googleapis.com/upload/drive/v3";

/// Google Drive file metadata
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DriveFile {
    pub id: String,
    pub name: String,
    #[serde(rename = "mimeType")]
    pub mime_type: String,
    #[serde(rename = "modifiedTime", default)]
    pub modified_time: String,
    #[serde(default)]
    pub size: Option<String>,
}

#[derive(Debug, Deserialize)]
struct FileListResponse {
    files: Vec<DriveFile>,
    #[serde(rename = "nextPageToken")]
    next_page_token: Option<String>,
}

/// Google Drive API v3 client
pub struct GDriveClient {
    access_token: String,
}

impl GDriveClient {
    pub fn new(access_token: String) -> Self {
        Self { access_token }
    }

    /// Authenticated fetch helper
    async fn fetch(&self, method: &str, url: &str, body: Option<&JsValue>) -> Result<web_sys::Response, String> {
        let opts = web_sys::RequestInit::new();
        opts.set_method(method);
        if let Some(b) = body {
            opts.set_body(b);
        }

        let request = web_sys::Request::new_with_str_and_init(url, &opts)
            .map_err(|e| format!("Request creation failed: {:?}", e))?;

        request.headers()
            .set("Authorization", &format!("Bearer {}", self.access_token))
            .ok();

        let window = web_sys::window().ok_or("No window")?;
        let resp_value = JsFuture::from(window.fetch_with_request(&request))
            .await
            .map_err(|e| format!("Fetch failed: {:?}", e))?;

        let resp: web_sys::Response = resp_value.dyn_into().map_err(|_| "Not a Response")?;

        if !resp.ok() {
            let status = resp.status();
            return Err(format!("Drive API error: {}", status));
        }

        Ok(resp)
    }

    async fn fetch_json<T: for<'a> Deserialize<'a>>(&self, url: &str) -> Result<T, String> {
        let resp = self.fetch("GET", url, None).await?;
        let json = JsFuture::from(resp.text().map_err(|_| "No text")?)
            .await
            .map_err(|e| format!("Text failed: {:?}", e))?;
        let text = json.as_string().unwrap_or_default();
        serde_json::from_str(&text).map_err(|e| format!("JSON parse failed: {}", e))
    }

    /// List files in a folder (or root if folder_id is "root")
    pub async fn list_files(&self, folder_id: &str) -> Result<Vec<DriveFile>, String> {
        let query = format!("'{}' in parents and trashed = false", folder_id);
        let url = format!(
            "{}/files?q={}&fields=files(id,name,mimeType,modifiedTime,size)&pageSize=100",
            DRIVE_API,
            js_sys::encode_uri_component(&query),
        );

        let response: FileListResponse = self.fetch_json(&url).await?;
        Ok(response.files)
    }

    /// Download file content as bytes
    pub async fn download_file(&self, file_id: &str) -> Result<Vec<u8>, String> {
        let url = format!("{}/files/{}?alt=media", DRIVE_API, file_id);
        let resp = self.fetch("GET", &url, None).await?;

        let buf = JsFuture::from(resp.array_buffer().map_err(|_| "No buffer")?)
            .await
            .map_err(|e| format!("Buffer failed: {:?}", e))?;
        let uint8 = js_sys::Uint8Array::new(&buf);
        Ok(uint8.to_vec())
    }

    /// Upload a new file to a folder
    pub async fn upload_file(&self, name: &str, folder_id: &str, data: &[u8], mime_type: &str) -> Result<DriveFile, String> {
        // Use multipart upload
        let metadata = serde_json::json!({
            "name": name,
            "parents": [folder_id],
        });

        let boundary = "typst_studio_boundary";

        let mut body = format!(
            "--{}\r\nContent-Type: application/json; charset=UTF-8\r\n\r\n{}\r\n--{}\r\nContent-Type: {}\r\n\r\n",
            boundary,
            metadata,
            boundary,
            mime_type
        ).into_bytes();
        body.extend_from_slice(data);
        body.extend_from_slice(format!("\r\n--{}--", boundary).as_bytes());

        let uint8 = js_sys::Uint8Array::from(&body[..]);
        let url = format!("{}/files?uploadType=multipart&fields=id,name,mimeType,modifiedTime", UPLOAD_API);

        let opts = web_sys::RequestInit::new();
        opts.set_method("POST");
        opts.set_body(&uint8);

        let request = web_sys::Request::new_with_str_and_init(&url, &opts)
            .map_err(|e| format!("Request creation failed: {:?}", e))?;

        request.headers()
            .set("Authorization", &format!("Bearer {}", self.access_token))
            .ok();
        request.headers()
            .set("Content-Type", &format!("multipart/related; boundary={}", boundary))
            .ok();

        let window = web_sys::window().ok_or("No window")?;
        let resp_value = JsFuture::from(window.fetch_with_request(&request))
            .await
            .map_err(|e| format!("Upload failed: {:?}", e))?;

        let resp: web_sys::Response = resp_value.dyn_into().map_err(|_| "Not a Response")?;
        if !resp.ok() {
            return Err(format!("Upload error: {}", resp.status()));
        }

        let text = JsFuture::from(resp.text().map_err(|_| "No text")?)
            .await
            .map_err(|e| format!("Text failed: {:?}", e))?;
        let text_str = text.as_string().unwrap_or_default();
        serde_json::from_str(&text_str).map_err(|e| format!("Parse failed: {}", e))
    }

    /// Update an existing file's content
    pub async fn update_file(&self, file_id: &str, data: &[u8], mime_type: &str) -> Result<(), String> {
        let url = format!("{}/files/{}?uploadType=media", UPLOAD_API, file_id);
        let uint8 = js_sys::Uint8Array::from(data);

        let opts = web_sys::RequestInit::new();
        opts.set_method("PATCH");
        opts.set_body(&uint8);

        let request = web_sys::Request::new_with_str_and_init(&url, &opts)
            .map_err(|e| format!("Request creation failed: {:?}", e))?;

        request.headers()
            .set("Authorization", &format!("Bearer {}", self.access_token))
            .ok();
        request.headers()
            .set("Content-Type", mime_type)
            .ok();

        let window = web_sys::window().ok_or("No window")?;
        let resp_value = JsFuture::from(window.fetch_with_request(&request))
            .await
            .map_err(|e| format!("Update failed: {:?}", e))?;

        let resp: web_sys::Response = resp_value.dyn_into().map_err(|_| "Not a Response")?;
        if !resp.ok() {
            return Err(format!("Update error: {}", resp.status()));
        }

        Ok(())
    }

    /// Delete a file
    pub async fn delete_file(&self, file_id: &str) -> Result<(), String> {
        let url = format!("{}/files/{}", DRIVE_API, file_id);
        let resp = self.fetch("DELETE", &url, None).await?;
        if !resp.ok() {
            return Err(format!("Delete error: {}", resp.status()));
        }
        Ok(())
    }

    /// Create a folder in Drive
    pub async fn create_folder(&self, name: &str, parent_id: &str) -> Result<DriveFile, String> {
        let metadata = serde_json::json!({
            "name": name,
            "mimeType": "application/vnd.google-apps.folder",
            "parents": [parent_id],
        });

        let body_str = metadata.to_string();
        let body = JsValue::from_str(&body_str);

        let url = format!("{}/files?fields=id,name,mimeType,modifiedTime", DRIVE_API);

        let opts = web_sys::RequestInit::new();
        opts.set_method("POST");
        opts.set_body(&body);

        let request = web_sys::Request::new_with_str_and_init(&url, &opts)
            .map_err(|e| format!("Request creation failed: {:?}", e))?;

        request.headers()
            .set("Authorization", &format!("Bearer {}", self.access_token))
            .ok();
        request.headers()
            .set("Content-Type", "application/json")
            .ok();

        let window = web_sys::window().ok_or("No window")?;
        let resp_value = JsFuture::from(window.fetch_with_request(&request))
            .await
            .map_err(|e| format!("Create folder failed: {:?}", e))?;

        let resp: web_sys::Response = resp_value.dyn_into().map_err(|_| "Not a Response")?;
        if !resp.ok() {
            return Err(format!("Create folder error: {}", resp.status()));
        }

        let text = JsFuture::from(resp.text().map_err(|_| "No text")?)
            .await
            .map_err(|e| format!("Text failed: {:?}", e))?;
        let text_str = text.as_string().unwrap_or_default();
        serde_json::from_str(&text_str).map_err(|e| format!("Parse failed: {}", e))
    }
}
