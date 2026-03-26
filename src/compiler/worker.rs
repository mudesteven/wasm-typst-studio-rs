//! Web Worker bridge for off-thread Typst compilation.
//!
//! The worker reuses the same WASM binary and JS glue that Trunk generates.
//! Communication is via postMessage with JSON-serialized data.

use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use serde::{Serialize, Deserialize};
use std::collections::HashMap;

/// Request sent to the worker
#[derive(Serialize, Deserialize)]
pub struct CompileRequest {
    pub id: u32,
    pub source: String,
    pub main_file: String,
    pub file_contents: HashMap<String, String>,
    pub image_cache: HashMap<String, String>,
    /// Package sources as (namespace, name, version, path, content)
    pub pkg_sources: Vec<(String, String, String, String, String)>,
    /// Package binaries as (namespace, name, version, path, base64_data)
    pub pkg_binaries: Vec<(String, String, String, String, String)>,
}

/// Response from the worker
#[derive(Serialize, Deserialize)]
pub struct CompileResponse {
    pub id: u32,
    pub svg: Option<String>,
    pub pdf_base64: Option<String>,
    pub error: Option<String>,
}

/// Called from the worker JS when a compile message arrives.
/// This runs on the WORKER thread, so it doesn't block the UI.
#[wasm_bindgen]
pub fn compile_in_worker(request_json: &str) -> String {
    use typst_syntax::package::{PackageSpec, PackageVersion};
    use ecow::EcoString;

    let req: CompileRequest = match serde_json::from_str(request_json) {
        Ok(r) => r,
        Err(e) => {
            let resp = CompileResponse {
                id: 0, svg: None, pdf_base64: None,
                error: Some(format!("Failed to parse request: {}", e)),
            };
            return serde_json::to_string(&resp).unwrap_or_default();
        }
    };

    // Reconstruct PackageSpec tuples
    let pkg_sources: Vec<(PackageSpec, String, String)> = req.pkg_sources.iter().map(|(ns, name, ver, path, content)| {
        let parts: Vec<u32> = ver.split('.').filter_map(|s| s.parse().ok()).collect();
        let spec = PackageSpec {
            namespace: EcoString::from(ns.as_str()),
            name: EcoString::from(name.as_str()),
            version: PackageVersion {
                major: *parts.first().unwrap_or(&0),
                minor: *parts.get(1).unwrap_or(&0),
                patch: *parts.get(2).unwrap_or(&0),
            },
        };
        (spec, path.clone(), content.clone())
    }).collect();

    let pkg_binaries: Vec<(PackageSpec, String, Vec<u8>)> = req.pkg_binaries.iter().filter_map(|(ns, name, ver, path, b64)| {
        let parts: Vec<u32> = ver.split('.').filter_map(|s| s.parse().ok()).collect();
        let spec = PackageSpec {
            namespace: EcoString::from(ns.as_str()),
            name: EcoString::from(name.as_str()),
            version: PackageVersion {
                major: *parts.first().unwrap_or(&0),
                minor: *parts.get(1).unwrap_or(&0),
                patch: *parts.get(2).unwrap_or(&0),
            },
        };
        let bytes = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, b64).ok()?;
        Some((spec, path.clone(), bytes))
    }).collect();

    let result = super::TypstCompiler::new()
        .and_then(|c| c.compile_to_both(
            &req.source, &req.main_file,
            &req.file_contents, &req.image_cache,
            &pkg_sources, &pkg_binaries,
        ));

    let resp = match result {
        Ok((svg, pdf)) => CompileResponse {
            id: req.id,
            svg: Some(svg),
            pdf_base64: Some(base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &pdf)),
            error: None,
        },
        Err(e) => CompileResponse {
            id: req.id, svg: None, pdf_base64: None,
            error: Some(e),
        },
    };

    serde_json::to_string(&resp).unwrap_or_default()
}

/// Manages the compile worker from the main thread
pub struct CompileWorkerHandle {
    worker: web_sys::Worker,
    _on_message: Closure<dyn FnMut(web_sys::MessageEvent)>,
}

impl CompileWorkerHandle {
    /// Spawn the compile worker. Returns None if workers aren't supported.
    pub fn spawn(on_result: impl Fn(CompileResponse) + 'static) -> Option<Self> {
        // Create inline worker script that loads our WASM module
        // The worker.js imports the same wasm-bindgen JS glue and calls compile_in_worker
        let worker_code = r#"
            let wasmModule = null;

            self.onmessage = async function(e) {
                const msg = e.data;

                if (msg.type === 'init') {
                    try {
                        // Dynamically import the wasm-bindgen JS glue
                        wasmModule = await import(msg.jsUrl);
                        await wasmModule.default(msg.wasmUrl);
                        self.postMessage({ type: 'ready' });
                    } catch(err) {
                        self.postMessage({ type: 'error', error: String(err) });
                    }
                    return;
                }

                if (msg.type === 'compile') {
                    try {
                        const result = wasmModule.compile_in_worker(msg.requestJson);
                        self.postMessage({ type: 'result', resultJson: result });
                    } catch(err) {
                        self.postMessage({ type: 'result', resultJson: JSON.stringify({
                            id: 0, svg: null, pdf_base64: null, error: String(err)
                        })});
                    }
                    return;
                }
            };
        "#;

        let blob_parts = js_sys::Array::new();
        blob_parts.push(&JsValue::from_str(worker_code));
        let mut opts = web_sys::BlobPropertyBag::new();
        opts.type_("application/javascript");
        let blob = web_sys::Blob::new_with_str_sequence_and_options(&blob_parts, &opts).ok()?;
        let url = web_sys::Url::create_object_url_with_blob(&blob).ok()?;

        let mut worker_opts = web_sys::WorkerOptions::new();
        worker_opts.type_(web_sys::WorkerType::Module);
        let worker = web_sys::Worker::new_with_options(&url, &worker_opts).ok()?;

        let _ = web_sys::Url::revoke_object_url(&url);

        // Handle messages from worker
        let on_message = Closure::wrap(Box::new(move |ev: web_sys::MessageEvent| {
            let data = ev.data();
            if let Some(obj) = data.dyn_ref::<js_sys::Object>() {
                let msg_type = js_sys::Reflect::get(obj, &JsValue::from_str("type"))
                    .ok().and_then(|v| v.as_string()).unwrap_or_default();

                match msg_type.as_str() {
                    "ready" => {
                        log::info!("Compile worker ready");
                    }
                    "result" => {
                        if let Some(json) = js_sys::Reflect::get(obj, &JsValue::from_str("resultJson"))
                            .ok().and_then(|v| v.as_string())
                        {
                            if let Ok(resp) = serde_json::from_str::<CompileResponse>(&json) {
                                on_result(resp);
                            }
                        }
                    }
                    "error" => {
                        let err = js_sys::Reflect::get(obj, &JsValue::from_str("error"))
                            .ok().and_then(|v| v.as_string()).unwrap_or_default();
                        log::error!("Worker error: {}", err);
                    }
                    _ => {}
                }
            }
        }) as Box<dyn FnMut(_)>);

        worker.set_onmessage(Some(on_message.as_ref().unchecked_ref()));

        // Initialize worker with WASM URLs
        // Trunk generates files like: wasm-typst-studio-rs-HASH.js and _bg.wasm
        // We need to find the actual URLs
        if let Some(window) = web_sys::window() {
            let base = window.location().origin().unwrap_or_default();
            // Find the JS and WASM file URLs from the document's scripts
            if let Some(document) = window.document() {
                // The Trunk-generated JS is loaded as a module script
                // We can find it by looking for script tags or by convention
                let scripts = document.query_selector_all("script[type='module']").ok();
                let mut js_url = String::new();
                if let Some(scripts) = scripts {
                    for i in 0..scripts.length() {
                        if let Some(el) = scripts.item(i) {
                            if let Some(src) = el.dyn_ref::<web_sys::HtmlScriptElement>() {
                                let s = src.src();
                                if s.contains("wasm-typst-studio") {
                                    js_url = s;
                                    break;
                                }
                            }
                        }
                    }
                }

                if js_url.is_empty() {
                    // Fallback: scan document for the JS URL pattern
                    let html = document.document_element().map(|e| e.inner_html()).unwrap_or_default();
                    if let Some(start) = html.find("wasm-typst-studio-rs-") {
                        if let Some(end) = html[start..].find(".js") {
                            let filename = &html[start..start + end + 3];
                            js_url = format!("{}/{}", base, filename);
                        }
                    }
                }

                if !js_url.is_empty() {
                    let wasm_url = js_url.replace(".js", "_bg.wasm");
                    let init_msg = js_sys::Object::new();
                    let _ = js_sys::Reflect::set(&init_msg, &"type".into(), &"init".into());
                    let _ = js_sys::Reflect::set(&init_msg, &"jsUrl".into(), &JsValue::from_str(&js_url));
                    let _ = js_sys::Reflect::set(&init_msg, &"wasmUrl".into(), &JsValue::from_str(&wasm_url));
                    let _ = worker.post_message(&init_msg);
                } else {
                    log::warn!("Could not find WASM JS glue URL for worker");
                }
            }
        }

        Some(Self { worker, _on_message: on_message })
    }

    /// Send a compile request to the worker
    pub fn compile(&self, request: &CompileRequest) {
        let json = match serde_json::to_string(request) {
            Ok(j) => j,
            Err(e) => { log::error!("Failed to serialize compile request: {}", e); return; }
        };
        let msg = js_sys::Object::new();
        let _ = js_sys::Reflect::set(&msg, &"type".into(), &"compile".into());
        let _ = js_sys::Reflect::set(&msg, &"requestJson".into(), &JsValue::from_str(&json));
        if let Err(e) = self.worker.post_message(&msg) {
            log::error!("Failed to post message to worker: {:?}", e);
        }
    }
}

/// Helper to convert package cache data to serializable format
pub fn serialize_pkg_sources(sources: &[(typst_syntax::package::PackageSpec, String, String)]) -> Vec<(String, String, String, String, String)> {
    sources.iter().map(|(spec, path, content)| {
        (
            spec.namespace.to_string(),
            spec.name.to_string(),
            spec.version.to_string(),
            path.clone(),
            content.clone(),
        )
    }).collect()
}

pub fn serialize_pkg_binaries(binaries: &[(typst_syntax::package::PackageSpec, String, Vec<u8>)]) -> Vec<(String, String, String, String, String)> {
    binaries.iter().map(|(spec, path, data)| {
        (
            spec.namespace.to_string(),
            spec.name.to_string(),
            spec.version.to_string(),
            path.clone(),
            base64::Engine::encode(&base64::engine::general_purpose::STANDARD, data),
        )
    }).collect()
}
