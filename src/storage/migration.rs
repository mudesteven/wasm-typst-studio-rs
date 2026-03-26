use crate::models::FileContent;
use base64::Engine as _;
use crate::storage::traits::ProjectStorage;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use web_sys::IdbDatabase;

/// Migrate data from the old storage format (localStorage + IndexedDB v1)
/// to the new project-based IndexedDB v2 format.
///
/// This checks if migration has already been done (via a localStorage flag)
/// and if old data exists, creates a "Default Project" with the migrated files.
pub async fn migrate_if_needed(storage: &dyn ProjectStorage) -> Result<Option<String>, String> {
    // Check if already migrated
    let window = web_sys::window().ok_or("No window")?;
    let ls = window.local_storage().map_err(|_| "No localStorage")?.ok_or("No localStorage")?;

    if let Ok(Some(_)) = ls.get_item("migration_v2_done") {
        return Ok(None);
    }

    // Check if there's old data to migrate
    let old_source = ls.get_item("typst_source").ok().flatten();
    let old_bib = ls.get_item("typst_bibliography").ok().flatten();
    let old_images = load_old_images().await;

    let has_old_data = old_source.is_some() || old_bib.is_some() || !old_images.is_empty();

    if !has_old_data {
        // No old data, just mark as done
        let _ = ls.set_item("migration_v2_done", "true");
        return Ok(None);
    }

    log::info!("Migrating old data to new project format...");

    // Create a "Default Project"
    let project = storage.create_project("My Project").await?;

    // Migrate source
    if let Some(source) = old_source {
        storage.write_file(&project.id, "main.typ", &FileContent::Text(source)).await?;
        log::info!("Migrated main.typ");
    }

    // Migrate bibliography
    if let Some(bib) = old_bib {
        storage.write_file(&project.id, "refs.yml", &FileContent::Text(bib)).await?;
        log::info!("Migrated refs.yml");
    }

    // Migrate images from old IndexedDB
    for (id, data) in &old_images {
        // Parse old image metadata JSON to extract actual image data
        if let Some((filename, image_data)) = parse_old_image_metadata(data) {
            let path = if filename.is_empty() {
                format!("images/{}.png", id)
            } else {
                format!("images/{}", filename)
            };

            // The image_data is a base64 data URL, extract raw bytes
            let bytes = decode_data_url(&image_data);
            if !bytes.is_empty() {
                storage.write_file(&project.id, &path, &FileContent::Binary(bytes)).await?;
                log::info!("Migrated image: {}", path);
            }
        }
    }

    // Mark migration as done
    let _ = ls.set_item("migration_v2_done", "true");

    // Store last project ID
    let _ = ls.set_item("last_project_id", &project.id);

    log::info!("Migration complete. Created project: {} ({})", project.name, project.id);
    Ok(Some(project.id))
}

/// Load images from old IndexedDB (typst_studio_db, images store)
async fn load_old_images() -> Vec<(String, String)> {
    let result = load_old_images_inner().await;
    result.unwrap_or_default()
}

async fn load_old_images_inner() -> Result<Vec<(String, String)>, String> {
    let window = web_sys::window().ok_or("No window")?;
    let idb_factory = window.indexed_db().map_err(|_| "No IDB")?.ok_or("No IDB")?;

    // Try to open old DB (don't create if it doesn't exist — use version 1)
    let open_request = idb_factory.open_with_u32("typst_studio_db", 1)
        .map_err(|e| format!("{:?}", e))?;

    let promise = js_sys::Promise::new(&mut |resolve, reject| {
        let req = open_request.clone();
        let onsuccess = Closure::wrap(Box::new(move |_: web_sys::Event| {
            if let Ok(result) = req.result() {
                let _ = resolve.call1(&JsValue::NULL, &result);
            }
        }) as Box<dyn FnMut(_)>);
        let onerror = Closure::wrap(Box::new(move |_: web_sys::Event| {
            let _ = reject.call1(&JsValue::NULL, &JsValue::from_str("Failed"));
        }) as Box<dyn FnMut(_)>);
        open_request.set_onsuccess(Some(onsuccess.as_ref().unchecked_ref()));
        open_request.set_onerror(Some(onerror.as_ref().unchecked_ref()));
        onsuccess.forget();
        onerror.forget();
    });

    let result = JsFuture::from(promise).await.map_err(|e| format!("{:?}", e))?;
    let db: IdbDatabase = result.dyn_into().map_err(|_| "Not a DB")?;

    // Check if store exists
    let store_names = db.object_store_names();
    if !store_names.contains("images") {
        return Ok(Vec::new());
    }

    let tx = db.transaction_with_str("images").map_err(|e| format!("{:?}", e))?;
    let store = tx.object_store("images").map_err(|e| format!("{:?}", e))?;

    let keys_req = store.get_all_keys().map_err(|e| format!("{:?}", e))?;
    let keys_promise = js_sys::Promise::new(&mut |resolve, reject| {
        let req = keys_req.clone();
        let onsuccess = Closure::wrap(Box::new(move |_: web_sys::Event| {
            if let Ok(r) = req.result() { let _ = resolve.call1(&JsValue::NULL, &r); }
        }) as Box<dyn FnMut(_)>);
        let onerror = Closure::wrap(Box::new(move |_: web_sys::Event| {
            let _ = reject.call1(&JsValue::NULL, &JsValue::from_str("Failed"));
        }) as Box<dyn FnMut(_)>);
        keys_req.set_onsuccess(Some(onsuccess.as_ref().unchecked_ref()));
        keys_req.set_onerror(Some(onerror.as_ref().unchecked_ref()));
        onsuccess.forget();
        onerror.forget();
    });

    let keys = JsFuture::from(keys_promise).await.map_err(|e| format!("{:?}", e))?;
    let keys_array: js_sys::Array = keys.dyn_into().map_err(|_| "Not an array")?;

    let mut images = Vec::new();
    for i in 0..keys_array.length() {
        if let Some(key) = keys_array.get(i).as_string() {
            let get_req = store.get(&JsValue::from_str(&key)).map_err(|e| format!("{:?}", e))?;
            let get_promise = js_sys::Promise::new(&mut |resolve, reject| {
                let req = get_req.clone();
                let onsuccess = Closure::wrap(Box::new(move |_: web_sys::Event| {
                    if let Ok(r) = req.result() { let _ = resolve.call1(&JsValue::NULL, &r); }
                }) as Box<dyn FnMut(_)>);
                let onerror = Closure::wrap(Box::new(move |_: web_sys::Event| {
                    let _ = reject.call1(&JsValue::NULL, &JsValue::from_str("Failed"));
                }) as Box<dyn FnMut(_)>);
                get_req.set_onsuccess(Some(onsuccess.as_ref().unchecked_ref()));
                get_req.set_onerror(Some(onerror.as_ref().unchecked_ref()));
                onsuccess.forget();
                onerror.forget();
            });
            if let Ok(val) = JsFuture::from(get_promise).await {
                if let Some(data) = val.as_string() {
                    images.push((key, data));
                }
            }
        }
    }

    Ok(images)
}

/// Parse old image metadata JSON: {"id":"001","filename":"img.png","data":"data:...","timestamp":123}
fn parse_old_image_metadata(json: &str) -> Option<(String, String)> {
    let parsed = js_sys::JSON::parse(json).ok()?;
    let obj = parsed.dyn_into::<js_sys::Object>().ok()?;

    let filename = js_sys::Reflect::get(&obj, &JsValue::from_str("filename"))
        .ok()
        .and_then(|v| v.as_string())
        .unwrap_or_default();

    let data = js_sys::Reflect::get(&obj, &JsValue::from_str("data"))
        .ok()
        .and_then(|v| v.as_string())
        .unwrap_or_default();

    if data.is_empty() {
        None
    } else {
        Some((filename, data))
    }
}

/// Decode a data URL (data:image/png;base64,...) to raw bytes
fn decode_data_url(data_url: &str) -> Vec<u8> {
    if let Some(comma_pos) = data_url.find(',') {
        let b64 = &data_url[comma_pos + 1..];
        base64::engine::general_purpose::STANDARD.decode(b64).unwrap_or_default()
    } else {
        Vec::new()
    }
}

/// Create the built-in "SS Notes" demo project if it doesn't already exist.
/// This project persists across sessions and serves as a sample.
pub async fn ensure_demo_project(storage: &dyn ProjectStorage) -> Result<(), String> {
    // Check if demo was already created
    let window = web_sys::window().ok_or("No window")?;
    let ls = window.local_storage().map_err(|_| "No localStorage")?.ok_or("No localStorage")?;
    if let Ok(Some(_)) = ls.get_item("demo_ss_notes_created") {
        return Ok(());
    }

    log::info!("Creating SS Notes demo project...");

    let project = storage.create_project("SS Notes (Demo)").await?;

    // Bundle all .typ files from the ss-notes lectures
    let files: &[(&str, &str)] = &[
        ("lec.typ", include_str!("../../examples/ss-notes/lec.typ")),
        ("lib.typ", include_str!("../../examples/ss-notes/lib.typ")),
        ("math.typ", include_str!("../../examples/ss-notes/math.typ")),
        ("intro.typ", include_str!("../../examples/ss-notes/intro.typ")),
        ("lti.typ", include_str!("../../examples/ss-notes/lti.typ")),
        ("fourier_series.typ", include_str!("../../examples/ss-notes/fourier_series.typ")),
        ("ct_fourier_transform.typ", include_str!("../../examples/ss-notes/ct_fourier_transform.typ")),
        ("dt_fourier_transform.typ", include_str!("../../examples/ss-notes/dt_fourier_transform.typ")),
        ("sampling.typ", include_str!("../../examples/ss-notes/sampling.typ")),
        ("laplace.typ", include_str!("../../examples/ss-notes/laplace.typ")),
        ("random_process.typ", include_str!("../../examples/ss-notes/random_process.typ")),
    ];

    for (path, content) in files {
        storage.write_file(&project.id, path, &FileContent::Text(content.to_string())).await?;
    }

    // Set main_file to lec.typ (the entry point)
    let mut project_updated = storage.get_project(&project.id).await?;
    project_updated.main_file = "lec.typ".to_string();
    storage.update_project(&project_updated).await?;

    let _ = ls.set_item("demo_ss_notes_created", "true");
    log::info!("SS Notes demo project created: {}", project.id);
    Ok(())
}
