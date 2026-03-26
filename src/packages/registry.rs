use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use std::collections::HashMap;

const REGISTRY_URL: &str = "https://packages.typst.org";

/// Our own package spec (not typst_syntax::PackageSpec to avoid confusion)
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct PkgSpec {
    pub namespace: String,
    pub name: String,
    pub version: String,
}

impl PkgSpec {
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim_start_matches('@');
        let (ns_name, version) = s.split_once(':')?;
        let (namespace, name) = ns_name.split_once('/')?;
        Some(Self {
            namespace: namespace.to_string(),
            name: name.to_string(),
            version: version.to_string(),
        })
    }

    pub fn to_string(&self) -> String {
        format!("@{}/{}:{}", self.namespace, self.name, self.version)
    }

    pub fn tar_url(&self) -> String {
        format!("{}/{}/{}-{}.tar.gz", REGISTRY_URL, self.namespace, self.name, self.version)
    }
}

/// Download and extract a package. Returns (relative_path -> file_bytes).
pub async fn download_package(spec: &PkgSpec) -> Result<HashMap<String, Vec<u8>>, String> {
    let url = spec.tar_url();
    log::info!("Downloading: {} from {}", spec.to_string(), url);

    let compressed = fetch_bytes(&url).await
        .map_err(|e| format!("Download failed: {}. The Typst package registry may block browser requests (CORS). Try pre-downloading packages.", e))?;

    log::info!("Downloaded {} bytes, decompressing...", compressed.len());
    let tar_bytes = decompress_gzip(&compressed).await?;
    log::info!("Decompressed to {} bytes", tar_bytes.len());
    let files = parse_tar(&tar_bytes)?;
    log::info!("Extracted {} files", files.len());
    Ok(files)
}

async fn fetch_bytes(url: &str) -> Result<Vec<u8>, String> {
    let window = web_sys::window().ok_or("No window")?;
    let resp_value = JsFuture::from(window.fetch_with_str(url))
        .await
        .map_err(|e| format!("Fetch error: {:?}", e))?;
    let resp: web_sys::Response = resp_value.dyn_into().map_err(|_| "Not a Response")?;

    if !resp.ok() {
        return Err(format!("HTTP {}: package not found", resp.status()));
    }

    let buf = JsFuture::from(resp.array_buffer().map_err(|_| "No buffer")?)
        .await
        .map_err(|e| format!("Read error: {:?}", e))?;
    Ok(js_sys::Uint8Array::new(&buf).to_vec())
}

async fn decompress_gzip(data: &[u8]) -> Result<Vec<u8>, String> {
    let window = web_sys::window().ok_or("No window")?;

    // Check DecompressionStream availability
    let ds_exists = js_sys::Reflect::get(&window, &JsValue::from_str("DecompressionStream"))
        .ok().filter(|v| v.is_function()).is_some();
    if !ds_exists {
        return Err("DecompressionStream API not available".to_string());
    }

    let ds_ctor = js_sys::Reflect::get(&window, &JsValue::from_str("DecompressionStream"))
        .map_err(|_| "No DecompressionStream")?
        .dyn_into::<js_sys::Function>().map_err(|_| "Not a function")?;
    let args = js_sys::Array::of1(&JsValue::from_str("gzip"));
    let decompressor = js_sys::Reflect::construct(&ds_ctor, &args)
        .map_err(|e| format!("Constructor failed: {:?}", e))?;

    let uint8 = js_sys::Uint8Array::from(data);
    let parts = js_sys::Array::new();
    parts.push(&uint8);
    let blob = web_sys::Blob::new_with_u8_array_sequence(&parts)
        .map_err(|_| "Blob failed")?;
    let stream = blob.stream();

    let pipe_fn = js_sys::Reflect::get(&stream, &JsValue::from_str("pipeThrough"))
        .map_err(|_| "No pipeThrough")?
        .dyn_into::<js_sys::Function>().map_err(|_| "Not function")?;
    let decompressed = pipe_fn.call1(&stream, &decompressor)
        .map_err(|e| format!("pipeThrough failed: {:?}", e))?;

    let response = web_sys::Response::new_with_opt_readable_stream(
        Some(&decompressed.dyn_into().map_err(|_| "Not ReadableStream")?)
    ).map_err(|_| "Response failed")?;

    let buf = JsFuture::from(response.array_buffer().map_err(|_| "No buffer")?)
        .await.map_err(|e| format!("Read failed: {:?}", e))?;
    Ok(js_sys::Uint8Array::new(&buf).to_vec())
}

/// Minimal tar parser
fn parse_tar(data: &[u8]) -> Result<HashMap<String, Vec<u8>>, String> {
    let mut files = HashMap::new();
    let mut pos = 0;

    while pos + 512 <= data.len() {
        let header = &data[pos..pos + 512];
        if header.iter().all(|&b| b == 0) { break; }

        // Filename (first 100 bytes)
        let name_end = header[..100].iter().position(|&b| b == 0).unwrap_or(100);
        let mut name = std::str::from_utf8(&header[..name_end])
            .map_err(|_| "Invalid filename")?.to_string();

        // UStar prefix (bytes 345-500)
        let prefix_end = header[345..500].iter().position(|&b| b == 0).unwrap_or(155);
        if prefix_end > 0 {
            if let Ok(prefix) = std::str::from_utf8(&header[345..345 + prefix_end]) {
                if !prefix.is_empty() {
                    name = format!("{}/{}", prefix, name);
                }
            }
        }

        // Clean up path: remove leading "./" or "/"
        if name.starts_with("./") {
            name = name[2..].to_string();
        } else if name.starts_with('/') {
            name = name[1..].to_string();
        }

        // File size (octal, bytes 124-135)
        let size_str = std::str::from_utf8(&header[124..135])
            .map_err(|_| "Invalid size")?
            .trim_matches(|c: char| c == '\0' || c == ' ');
        let size = usize::from_str_radix(size_str, 8).unwrap_or(0);

        let file_type = header[156];
        pos += 512;

        if (file_type == b'0' || file_type == 0) && size > 0 && !name.is_empty() {
            if pos + size <= data.len() {
                files.insert(name, data[pos..pos + size].to_vec());
            }
        }

        pos += (size + 511) & !511;
    }

    Ok(files)
}

/// Extract package specs from error text
pub fn extract_missing_packages(error: &str) -> Vec<PkgSpec> {
    let mut packages = Vec::new();
    for word in error.split_whitespace() {
        let clean = word.trim_matches(|c: char| !c.is_alphanumeric() && c != '@' && c != '/' && c != ':' && c != '.' && c != '-');
        if clean.starts_with("@") && clean.contains('/') && clean.contains(':') {
            if let Some(spec) = PkgSpec::parse(clean) {
                if !packages.contains(&spec) {
                    packages.push(spec);
                }
            }
        }
    }
    packages
}
