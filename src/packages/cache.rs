use super::registry::PkgSpec;
use std::collections::HashMap;
use leptos::prelude::*;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use web_sys::{IdbDatabase, IdbTransactionMode};
use base64::Engine as _;
use typst_syntax::package::{PackageSpec, PackageVersion};
use ecow::EcoString;

const DB_NAME: &str = "typst_studio_packages";
const DB_VERSION: u32 = 2; // Bumped to clear stale caches from old tar parser
const STORE_NAME: &str = "packages";

/// In-memory cache of downloaded package files.
#[derive(Clone)]
pub struct PackageCache {
    pub packages: RwSignal<HashMap<String, HashMap<String, Vec<u8>>>>,
}

impl PackageCache {
    pub fn new() -> Self {
        Self { packages: RwSignal::new(HashMap::new()) }
    }

    pub fn has_package(&self, spec: &PkgSpec) -> bool {
        self.packages.get_untracked().contains_key(&spec.to_string())
    }

    fn make_typst_pkg_spec(spec: &PkgSpec) -> PackageSpec {
        let parts: Vec<u32> = spec.version.split('.').filter_map(|s| s.parse().ok()).collect();
        PackageSpec {
            namespace: EcoString::from(spec.namespace.as_str()),
            name: EcoString::from(spec.name.as_str()),
            version: PackageVersion {
                major: *parts.first().unwrap_or(&0),
                minor: *parts.get(1).unwrap_or(&0),
                patch: *parts.get(2).unwrap_or(&0),
            },
        }
    }

    /// Get all .typ source files for the compiler.
    /// Returns (PackageSpec, path, content) triples.
    pub fn get_all_sources(&self) -> Vec<(PackageSpec, String, String)> {
        let pkgs = self.packages.get_untracked();
        let mut files = Vec::new();
        for (spec_str, pkg_files) in &pkgs {
            if let Some(spec) = PkgSpec::parse(spec_str) {
                let ts = Self::make_typst_pkg_spec(&spec);
                for (path, bytes) in pkg_files {
                    if path.ends_with(".typ") {
                        if let Ok(content) = std::str::from_utf8(bytes) {
                            files.push((ts.clone(), path.clone(), content.to_string()));
                        }
                    }
                }
            }
        }
        files
    }

    /// Get all binary files for the compiler.
    /// Returns (PackageSpec, path, bytes) triples.
    pub fn get_all_binaries(&self) -> Vec<(PackageSpec, String, Vec<u8>)> {
        let pkgs = self.packages.get_untracked();
        let mut files = Vec::new();
        for (spec_str, pkg_files) in &pkgs {
            if let Some(spec) = PkgSpec::parse(spec_str) {
                let ts = Self::make_typst_pkg_spec(&spec);
                for (path, bytes) in pkg_files {
                    if !path.ends_with(".typ") {
                        files.push((ts.clone(), path.clone(), bytes.clone()));
                    }
                }
            }
        }
        files
    }

    pub async fn store_package(&self, spec: &PkgSpec, files: HashMap<String, Vec<u8>>) -> Result<(), String> {
        let key = spec.to_string();
        self.packages.update(|pkgs| { pkgs.insert(key.clone(), files.clone()); });

        let db = Self::open_db().await?;
        let tx = db.transaction_with_str_and_mode(STORE_NAME, IdbTransactionMode::Readwrite)
            .map_err(|e| format!("{:?}", e))?;
        let store = tx.object_store(STORE_NAME).map_err(|e| format!("{:?}", e))?;
        let serialized: HashMap<String, String> = files.iter()
            .map(|(k, v)| (k.clone(), base64::engine::general_purpose::STANDARD.encode(v)))
            .collect();
        let json = serde_json::to_string(&serialized).map_err(|e| format!("{}", e))?;
        let request = store.put_with_key(&JsValue::from_str(&json), &JsValue::from_str(&key))
            .map_err(|e| format!("{:?}", e))?;
        Self::await_request(&request).await?;
        Ok(())
    }

    pub async fn remove_package(&self, spec: &PkgSpec) -> Result<(), String> {
        let key = spec.to_string();
        self.packages.update(|pkgs| { pkgs.remove(&key); });
        let db = Self::open_db().await?;
        let tx = db.transaction_with_str_and_mode(STORE_NAME, IdbTransactionMode::Readwrite)
            .map_err(|e| format!("{:?}", e))?;
        let store = tx.object_store(STORE_NAME).map_err(|e| format!("{:?}", e))?;
        let request = store.delete(&JsValue::from_str(&key)).map_err(|e| format!("{:?}", e))?;
        Self::await_request(&request).await?;
        Ok(())
    }

    pub async fn load_all(&self) -> Result<(), String> {
        let db = Self::open_db().await?;
        let tx = db.transaction_with_str(STORE_NAME).map_err(|e| format!("{:?}", e))?;
        let store = tx.object_store(STORE_NAME).map_err(|e| format!("{:?}", e))?;
        let keys_req = store.get_all_keys().map_err(|e| format!("{:?}", e))?;
        let keys = Self::await_request(&keys_req).await?;
        let keys_array: js_sys::Array = keys.dyn_into().map_err(|_| "Not an array")?;
        let mut loaded = HashMap::new();
        let mut stale_keys = Vec::new();
        for i in 0..keys_array.length() {
            if let Some(key) = keys_array.get(i).as_string() {
                let get_req = store.get(&JsValue::from_str(&key)).map_err(|e| format!("{:?}", e))?;
                let val = Self::await_request(&get_req).await?;
                if let Some(json_str) = val.as_string() {
                    if let Ok(serialized) = serde_json::from_str::<HashMap<String, String>>(&json_str) {
                        let files: HashMap<String, Vec<u8>> = serialized.into_iter()
                            .filter_map(|(k, v)| {
                                base64::engine::general_purpose::STANDARD.decode(&v).ok().map(|b| (k, b))
                            }).collect();
                        // Validate: packages must have typst.toml (stale cache from old tar parser won't)
                        if files.contains_key("typst.toml") {
                            log::info!("  Package {} has {} files (valid)", key, files.len());
                            loaded.insert(key, files);
                        } else {
                            log::warn!("  Package {} has {} files but missing typst.toml - stale cache, will remove", key, files.len());
                            stale_keys.push(key);
                        }
                    }
                }
            }
        }
        // Remove stale packages from IndexedDB
        if !stale_keys.is_empty() {
            log::info!("Removing {} stale cached packages", stale_keys.len());
            let db2 = Self::open_db().await?;
            let tx2 = db2.transaction_with_str_and_mode(STORE_NAME, IdbTransactionMode::Readwrite)
                .map_err(|e| format!("{:?}", e))?;
            let store2 = tx2.object_store(STORE_NAME).map_err(|e| format!("{:?}", e))?;
            for key in &stale_keys {
                let _ = store2.delete(&JsValue::from_str(key));
            }
        }
        let count = loaded.len();
        self.packages.set(loaded);
        log::info!("Loaded {} cached packages ({} stale removed)", count, stale_keys.len());
        Ok(())
    }

    pub fn list_packages(&self) -> Vec<PkgSpec> {
        self.packages.get().keys().filter_map(|k| PkgSpec::parse(k)).collect()
    }

    async fn open_db() -> Result<IdbDatabase, String> {
        let window = web_sys::window().ok_or("No window")?;
        let factory = window.indexed_db().map_err(|_| "No IDB")?.ok_or("No IDB")?;
        let open_req = factory.open_with_u32(DB_NAME, DB_VERSION).map_err(|e| format!("{:?}", e))?;
        let onupgrade = wasm_bindgen::closure::Closure::wrap(Box::new(move |ev: web_sys::IdbVersionChangeEvent| {
            if let Some(t) = ev.target() {
                if let Ok(r) = t.dyn_into::<web_sys::IdbOpenDbRequest>() {
                    if let Ok(res) = r.result() {
                        if let Ok(db) = res.dyn_into::<IdbDatabase>() {
                            // On upgrade from v1: delete old store with stale paths
                            if db.object_store_names().contains(STORE_NAME) {
                                let _ = db.delete_object_store(STORE_NAME);
                                log::info!("Cleared stale package cache (DB upgrade to v{})", DB_VERSION);
                            }
                            let _ = db.create_object_store(STORE_NAME);
                        }
                    }
                }
            }
        }) as Box<dyn FnMut(_)>);
        open_req.set_onupgradeneeded(Some(onupgrade.as_ref().unchecked_ref()));
        onupgrade.forget();
        let promise = js_sys::Promise::new(&mut |resolve, reject| {
            let req = open_req.clone();
            let ok = wasm_bindgen::closure::Closure::wrap(Box::new(move |_: web_sys::Event| {
                if let Ok(r) = req.result() { let _ = resolve.call1(&JsValue::NULL, &r); }
            }) as Box<dyn FnMut(_)>);
            let err = wasm_bindgen::closure::Closure::wrap(Box::new(move |_: web_sys::Event| {
                let _ = reject.call1(&JsValue::NULL, &JsValue::from_str("DB open failed"));
            }) as Box<dyn FnMut(_)>);
            open_req.set_onsuccess(Some(ok.as_ref().unchecked_ref()));
            open_req.set_onerror(Some(err.as_ref().unchecked_ref()));
            ok.forget(); err.forget();
        });
        JsFuture::from(promise).await.map_err(|e| format!("{:?}", e))?
            .dyn_into::<IdbDatabase>().map_err(|_| "Not a DB".to_string())
    }

    async fn await_request(request: &web_sys::IdbRequest) -> Result<JsValue, String> {
        let promise = {
            let req = request.clone();
            js_sys::Promise::new(&mut |resolve, reject| {
                let r = req.clone();
                let ok = wasm_bindgen::closure::Closure::wrap(Box::new(move |_: web_sys::Event| {
                    if let Ok(res) = r.result() { let _ = resolve.call1(&JsValue::NULL, &res); }
                }) as Box<dyn FnMut(_)>);
                let err = wasm_bindgen::closure::Closure::wrap(Box::new(move |_: web_sys::Event| {
                    let _ = reject.call1(&JsValue::NULL, &JsValue::from_str("IDB failed"));
                }) as Box<dyn FnMut(_)>);
                req.set_onsuccess(Some(ok.as_ref().unchecked_ref()));
                req.set_onerror(Some(err.as_ref().unchecked_ref()));
                ok.forget(); err.forget();
            })
        };
        JsFuture::from(promise).await.map_err(|e| format!("{:?}", e))
    }
}
