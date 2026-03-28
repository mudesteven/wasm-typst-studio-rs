use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_meta::*;
use gloo_timers::future::sleep;
use std::time::Duration;
use std::collections::HashMap;
use std::sync::Arc;
use wasm_bindgen::JsCast;

mod components;
mod compiler;
mod models;
mod packages;
mod state;
mod storage;
mod sync;
mod utils;

use crate::components::{
    Editor, Preview, ImageGalleryDrawer, ProjectSwitcher, FileSidebar,
    EditorTabs, HomePage,
};
use crate::compiler::TypstCompiler;
use crate::state::AppState;
use crate::state::app_state::ThemeMode;
use crate::storage::create_storage;
use crate::components::project_manager::sync_source_from_state;

#[component]
pub fn App() -> impl IntoView {
    provide_meta_context();

    let storage = create_storage();
    let app_state = AppState::new(storage);
    provide_context(app_state.clone());

    // Theme: react to theme_mode signal and system preference changes
    {
        let app_state = app_state.clone();
        Effect::new(move |_| {
            let mode = app_state.theme_mode.get();
            let theme = mode.resolve();
            if let Some(window) = web_sys::window() {
                if let Some(document) = window.document() {
                    if let Some(html) = document.document_element() {
                        let _ = html.set_attribute("data-theme", theme);
                    }
                }
            }
        });
    }

    // Listen for system theme changes (for "System" mode)
    {
        let app_state = app_state.clone();
        spawn_local(async move {
            if let Some(window) = web_sys::window() {
                if let Ok(Some(mql)) = window.match_media("(prefers-color-scheme: dark)") {
                    let cb = wasm_bindgen::closure::Closure::wrap(Box::new(move |_: web_sys::Event| {
                        // Re-trigger theme effect by nudging the signal
                        let current = app_state.theme_mode.get_untracked();
                        if current == ThemeMode::System {
                            // Force reactivity by setting again
                            app_state.theme_mode.set(ThemeMode::System);
                        }
                    }) as Box<dyn FnMut(_)>);
                    let _ = mql.add_event_listener_with_callback("change", cb.as_ref().unchecked_ref());
                    cb.forget();
                }
            }
        });
    }

    // Editor state
    let (source, set_source) = signal(String::new());
    let (output, set_output) = signal(String::new());
    let (error, set_error) = signal(Option::<String>::None);
    let (is_compiling, set_is_compiling) = signal(false);
    let (pdf_blob_url, set_pdf_blob_url) = signal(Option::<String>::None);
    // Cursor scroll position: ratio 0.0-1.0 of where cursor is in the document
    let (cursor_ratio, set_cursor_ratio) = signal(0.0f64);
    let (editor_width, set_editor_width) = signal(50.0);
    let (is_resizing, set_is_resizing) = signal(false);
    use leptos::html::Textarea;
    let textarea_ref = NodeRef::<Textarea>::new();
    let (show_bib_modal, set_show_bib_modal) = signal(false);
    let (bib_content, set_bib_content) = signal(String::new());
    let (show_image_gallery, set_show_image_gallery) = signal(false);
    let (_legacy_image_cache, set_legacy_image_cache) = signal(HashMap::<String, String>::new());
    // Track init completion to prevent auto-save from clearing content on startup
    let (init_done, set_init_done) = signal(false);

    // Initialize
    {
        let app_state = app_state.clone();
        spawn_local(async move {
            let storage = app_state.storage.clone();
            match crate::storage::migration::migrate_if_needed(&**storage).await {
                Ok(Some(id)) => log::info!("Migration created project: {}", id),
                Ok(None) => {}
                Err(e) => log::error!("Migration failed: {}", e),
            }
            // Create demo project if needed
            if let Err(e) = crate::storage::migration::ensure_demo_project(&**storage).await {
                log::error!("Failed to create demo project: {}", e);
            }
            // Load cached packages
            if let Err(e) = app_state.package_cache.load_all().await {
                log::error!("Failed to load package cache: {}", e);
            }

            // Always start on homepage for now
            // Mark init complete so auto-save can start
            set_init_done.set(true);
        });
    }

    // Sync source when active file changes
    {
        let app_state = app_state.clone();
        Effect::new(move |_| {
            let active = app_state.active_file.get();
            if let Some(path) = active {
                let contents = app_state.file_contents.get();
                if let Some(content) = contents.get(&path) {
                    if source.get_untracked() != *content {
                        set_source.set(content.clone());
                    }
                }
            }
        });
    }

    // Auto-save (gated by autosave_enabled AND init_done)
    {
        let app_state = app_state.clone();
        Effect::new(move |_| {
            let src = source.get();
            if !init_done.get() { return; } // Don't save during initialization
            let autosave = app_state.autosave_enabled.get();
            let active = app_state.active_file.get_untracked();
            if let Some(path) = active {
                // Only update if content actually changed to avoid spurious modified markers
                let current = app_state.file_contents.get_untracked();
                if current.get(&path).map(|c| c == &src).unwrap_or(false) {
                    return;
                }
                app_state.set_file_content(&path, src.clone());
                // Only persist to storage if autosave is on
                if autosave {
                    if let Some(project) = app_state.current_project.get_untracked() {
                        let storage = app_state.storage.clone();
                        let app_state2 = app_state.clone();
                        let path = path.clone();
                        let src = src.clone();
                        spawn_local(async move {
                            let _ = storage.write_file(&project.id, &path,
                                &crate::models::FileContent::Text(src)).await;
                            app_state2.modified_files.update(|s| { s.remove(&path); });
                        });
                    }
                }
            }
        });
    }

    // Insert at cursor
    let insert_at_cursor = Arc::new(move |text: &str, select_text: Option<&str>| {
        if let Some(textarea) = textarea_ref.get() {
            let start = textarea.selection_start().unwrap_or(None).unwrap_or(0) as usize;
            let end = textarea.selection_end().unwrap_or(None).unwrap_or(0) as usize;
            let current = source.get_untracked();
            let before = &current[..start];
            let after = &current[end..];
            if start != end {
                let selected = &current[start..end];
                let new_text = text.replace("text", selected)
                    .replace("Heading", selected).replace("Item", selected)
                    .replace("formula", selected).replace("code", selected)
                    .replace("citation", selected).replace("label", selected);
                let new_source = format!("{}{}{}", before, new_text, after);
                set_source.set(new_source);
                let _ = textarea.set_selection_start(Some(start as u32));
                let _ = textarea.set_selection_end(Some((start + new_text.len()) as u32));
            } else {
                let new_source = format!("{}{}{}", before, text, after);
                set_source.set(new_source);
                if let Some(placeholder) = select_text {
                    if let Some(pos) = text.find(placeholder) {
                        let _ = textarea.set_selection_start(Some((start + pos) as u32));
                        let _ = textarea.set_selection_end(Some((start + pos + placeholder.len()) as u32));
                    }
                } else {
                    let _ = textarea.set_selection_start(Some((start + text.len()) as u32));
                }
            }
            let _ = textarea.focus();
        }
    });

    // Manual compile trigger
    let (compile_trigger, set_compile_trigger) = signal(0u32);

    // Manual save
    let on_save: Arc<dyn Fn() + Send + Sync> = {
        let app_state = app_state.clone();
        Arc::new(move || {
            let active = app_state.active_file.get_untracked();
            if let Some(path) = active {
                let src = source.get_untracked();
                if let Some(project) = app_state.current_project.get_untracked() {
                    let storage = app_state.storage.clone();
                    let app_state = app_state.clone();
                    let path_clone = path.clone();
                    spawn_local(async move {
                        let _ = storage.write_file(&project.id, &path,
                            &crate::models::FileContent::Text(src)).await;
                        // Clear modified indicator for this file
                        app_state.modified_files.update(|s| { s.remove(&path_clone); });
                        log::info!("Saved: {}", path);
                    });
                }
            }
        })
    };

    let on_compile: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
        set_compile_trigger.update(|t| *t += 1);
    });

    // Spawn the compile worker (off main thread)
    let compile_worker = {
        use crate::compiler::worker::{CompileWorkerHandle, CompileResponse};

        let set_output = set_output.clone();
        let set_error = set_error.clone();
        let set_pdf_blob_url = set_pdf_blob_url.clone();
        let app_state_w = app_state.clone();

        // The worker calls this callback on the main thread when compilation finishes
        let worker = CompileWorkerHandle::spawn(move |resp: CompileResponse| {
            if let Some(svg) = resp.svg {
                set_output.set(svg);
                set_error.set(None);

                // Decode PDF from base64 and create blob URL
                if let Some(pdf_b64) = resp.pdf_base64 {
                    if let Ok(pdf_bytes) = base64::Engine::decode(
                        &base64::engine::general_purpose::STANDARD, &pdf_b64
                    ) {
                        let uint8 = js_sys::Uint8Array::from(&pdf_bytes[..]);
                        let parts = js_sys::Array::new();
                        parts.push(&uint8);
                        let opts = web_sys::BlobPropertyBag::new();
                        opts.set_type("application/pdf");
                        if let Ok(blob) = web_sys::Blob::new_with_u8_array_sequence_and_options(&parts, &opts) {
                            if let Some(old) = pdf_blob_url.get_untracked() {
                                let _ = web_sys::Url::revoke_object_url(&old);
                            }
                            if let Ok(url) = web_sys::Url::create_object_url_with_blob(&blob) {
                                set_pdf_blob_url.set(Some(url));
                            }
                        }
                    }
                }
            } else if let Some(e) = resp.error {
                // Check for missing packages in ALL project files
                let src = source.get_untracked();
                let file_contents = app_state_w.file_contents.get_untracked();
                let pkg_cache = app_state_w.package_cache.clone();
                let missing = scan_all_missing_packages(&src, &file_contents, &pkg_cache);
                log::info!("Compilation error. Missing packages detected: {}", missing.len());
                for m in &missing { log::info!("  missing: {}", m.to_string()); }
                if !missing.is_empty() {
                    let pkg_names: Vec<String> = missing.iter().map(|s| s.to_string()).collect();
                    let msg = format!("{}\n\n[MISSING_PACKAGES:{}]", e, pkg_names.join(","));
                    set_error.set(Some(msg));
                } else {
                    set_error.set(Some(e));
                }
            }
            set_is_compiling.set(false);
        });

        if worker.is_some() {
            log::info!("Compile worker spawned — compilation will run off main thread");
        } else {
            log::warn!("Web Worker not available — compilation will block the UI");
        }
        std::rc::Rc::new(worker)
    };

    // Debounced compilation
    let (debounce_id, set_debounce_id) = signal(0u32);
    let (compile_id_counter, set_compile_id_counter) = signal(0u32);
    {
        let app_state = app_state.clone();
        Effect::new(move |_| {
            let src = source.get();
            let _trigger = compile_trigger.get();
            if src.is_empty() { return; }

            let current_id = debounce_id.get_untracked() + 1;
            set_debounce_id.set(current_id);

            // Read tracked signals for reactivity
            let _file_ver = app_state.file_contents.get();
            let _img_ver = app_state.image_cache.get();
            let _pkg_ver = app_state.package_cache.packages.get();
            // Always compile from the project's main file (entry point)
            let main_file = app_state.current_project.get_untracked()
                .map(|p| p.main_file).unwrap_or_else(|| "main.typ".to_string());

            let app_state2 = app_state.clone();
            let compile_worker = compile_worker.clone(); // Clone Rc for spawn_local

            spawn_local(async move {
                sleep(Duration::from_millis(500)).await;
                if current_id != debounce_id.get_untracked() { return; }

                set_is_compiling.set(true);

                // Clone data AFTER debounce so we get the latest content
                let file_contents = app_state2.file_contents.get_untracked();
                let image_cache = app_state2.image_cache.get_untracked();
                let pkg_sources = app_state2.package_cache.get_all_sources();
                let pkg_binaries = app_state2.package_cache.get_all_binaries();

                // Use the main file's latest content (autosave keeps file_contents current)
                let compile_src = file_contents.get(&main_file).cloned().unwrap_or(src);

                if let Some(ref worker) = *compile_worker {
                    // OFF-THREAD: send to worker, result comes back via callback
                    use crate::compiler::worker::{CompileRequest, serialize_pkg_sources, serialize_pkg_binaries};
                    let req_id = compile_id_counter.get_untracked() + 1;
                    set_compile_id_counter.set(req_id);
                    let request = CompileRequest {
                        id: req_id,
                        source: compile_src,
                        main_file,
                        file_contents,
                        image_cache,
                        pkg_sources: serialize_pkg_sources(&pkg_sources),
                        pkg_binaries: serialize_pkg_binaries(&pkg_binaries),
                    };
                    worker.compile(&request);
                    // Don't set_is_compiling(false) here — the callback does it
                } else {
                    // FALLBACK: compile on main thread (blocks UI)
                    let pkg_cache = app_state2.package_cache.clone();
                    match TypstCompiler::new()
                        .and_then(|c| c.compile_to_both(&compile_src, &main_file, &file_contents, &image_cache, &pkg_sources, &pkg_binaries))
                    {
                        Ok((svg, pdf_bytes)) => {
                            set_output.set(svg);
                            set_error.set(None);
                            let uint8 = js_sys::Uint8Array::from(&pdf_bytes[..]);
                            let parts = js_sys::Array::new();
                            parts.push(&uint8);
                            let opts = web_sys::BlobPropertyBag::new();
                            opts.set_type("application/pdf");
                            if let Ok(blob) = web_sys::Blob::new_with_u8_array_sequence_and_options(&parts, &opts) {
                                if let Some(old) = pdf_blob_url.get_untracked() {
                                    let _ = web_sys::Url::revoke_object_url(&old);
                                }
                                if let Ok(url) = web_sys::Url::create_object_url_with_blob(&blob) {
                                    set_pdf_blob_url.set(Some(url));
                                }
                            }
                        }
                        Err(e) => {
                            let missing = scan_all_missing_packages(&compile_src, &file_contents, &pkg_cache);
                            if !missing.is_empty() {
                                let pkg_names: Vec<String> = missing.iter().map(|s| s.to_string()).collect();
                                let msg = format!("{}\n\n[MISSING_PACKAGES:{}]", e, pkg_names.join(","));
                                set_error.set(Some(msg));
                            } else {
                                set_error.set(Some(e));
                            }
                        }
                    }
                    set_is_compiling.set(false);
                }
            });
        });
    }

    let has_project = move || app_state.current_project.get().is_some();
    let show_editor = move || app_state.current_project.get().is_some();
    let show_home = move || app_state.current_project.get().is_none();
    let app_state_pdf = app_state.clone();
    let app_state_bib = app_state.clone();
    let app_state_menu = app_state.clone();
    let on_save_menu = on_save.clone();

    view! {
        <Html attr:lang="en" attr:dir="ltr" attr:data-theme="dark" />
        <Title text="Typst Studio" />
        <Meta charset="UTF-8" />
        <Meta name="viewport" content="width=device-width, initial-scale=1.0" />

        <div class="flex flex-col h-screen">
            // Thin navbar
            <header class="flex items-center bg-base-200 border-b border-base-300 px-2 h-9 min-h-9 gap-1">
                <div class="flex items-center gap-1 flex-1 min-w-0">
                    // Logo + home (always first)
                    <button class="btn btn-ghost btn-xs px-1" title="Home"
                        on:click=move |_| {
                            app_state.current_project.set(None);
                            app_state.home_tab.set(crate::state::app_state::HomeTab::Projects);
                        }>
                        <span class="icon-[lucide--file-type] text-lg text-primary"></span>
                    </button>
                    <span class="text-sm font-bold hidden sm:inline">"Typst Studio"</span>

                    // Project switcher (always visible when project open)
                    {move || {
                        app_state.current_project.get().map(|p| {
                            let name = p.name.clone();
                            view! {
                                <button class="btn btn-ghost btn-xs gap-1 text-base-content/60 text-xs"
                                    on:click=move |_| app_state.show_project_manager.set(true)>
                                    <span class="icon-[lucide--folder] text-xs"></span>
                                    {name}
                                    <span class="icon-[lucide--chevron-down] text-[10px]"></span>
                                </button>
                            }
                        })
                    }}

                    // Menus (only in editor view, after project name)
                    <Show when=show_editor>
                        <div class="divider divider-horizontal mx-0 h-4"></div>
                        <div class="dropdown dropdown-hover">
                            <div tabindex="0" role="button" class="btn btn-ghost btn-xs px-2 text-xs font-normal">"File"</div>
                            <ul tabindex="0" class="dropdown-content menu bg-base-200 rounded-box z-50 w-52 p-1 shadow-lg border border-base-300">
                                <li><a class="text-xs" on:click={
                                    let save = on_save_menu.clone();
                                    move |_| save()
                                }>
                                    <span class="icon-[lucide--save] text-sm"></span>"Save"
                                </a></li>
                                <li><a class="text-xs" on:click=move |_| {
                                    let content = source.get();
                                    download_blob(&content, "text/plain;charset=utf-8", "document.typ");
                                }>
                                    <span class="icon-[lucide--file-down] text-sm"></span>"Save as .typ"
                                </a></li>
                                <li><a class="text-xs">
                                    <label class="flex items-center gap-2 cursor-pointer">
                                        <span class="icon-[lucide--upload] text-sm"></span>"Open .typ"
                                        <input type="file" accept=".typ" class="hidden"
                                            on:change=move |ev| {
                                                let target = ev.target().unwrap();
                                                let input = target.dyn_into::<web_sys::HtmlInputElement>().unwrap();
                                                if let Some(files) = input.files() {
                                                    if let Some(file) = files.get(0) {
                                                        let reader = web_sys::FileReader::new().unwrap();
                                                        let reader_clone = reader.clone();
                                                        let onload = wasm_bindgen::closure::Closure::wrap(
                                                            Box::new(move |_: web_sys::Event| {
                                                                if let Ok(result) = reader_clone.result() {
                                                                    if let Some(text) = result.as_string() {
                                                                        set_source.set(text);
                                                                    }
                                                                }
                                                            }) as Box<dyn FnMut(_)>,
                                                        );
                                                        reader.set_onload(Some(onload.as_ref().unchecked_ref()));
                                                        let _ = reader.read_as_text(&file);
                                                        onload.forget();
                                                    }
                                                }
                                            }
                                        />
                                    </label>
                                </a></li>
                                <div class="divider my-0"></div>
                                <li><a class="text-xs" on:click=move |_| {
                                    let svg = output.get();
                                    if !svg.is_empty() && error.get().is_none() {
                                        download_blob(&svg, "image/svg+xml", "document.svg");
                                    }
                                }>
                                    <span class="icon-[lucide--download] text-sm"></span>"Export SVG"
                                </a></li>
                                <li><a class="text-xs" on:click={
                                    let app_state = app_state_pdf.clone();
                                    move |_| {
                                        let src = source.get();
                                        if src.is_empty() || error.get().is_some() { return; }
                                        let fc = app_state.file_contents.get();
                                        let ic = app_state.image_cache.get();
                                        // Use active file as main_file
                                        let mf = app_state.active_file.get()
                                            .unwrap_or_else(|| {
                                                app_state.current_project.get()
                                                    .map(|p| p.main_file).unwrap_or_else(|| "main.typ".to_string())
                                            });
                                        let ps = app_state.package_cache.get_all_sources();
                                        let pb = app_state.package_cache.get_all_binaries();
                                        match TypstCompiler::new().and_then(|c| c.compile_to_pdf(&src, &mf, &fc, &ic, &ps, &pb)) {
                                            Ok(bytes) => download_bytes(&bytes, "application/pdf", "document.pdf"),
                                            Err(e) => { log::error!("PDF: {}", e); set_error.set(Some(e)); }
                                        }
                                    }
                                }>
                                    <span class="icon-[lucide--file-text] text-sm"></span>"Export PDF"
                                </a></li>
                            </ul>
                        </div>

                        <div class="dropdown dropdown-hover">
                            <div tabindex="0" role="button" class="btn btn-ghost btn-xs px-2 text-xs font-normal">"Edit"</div>
                            <ul tabindex="0" class="dropdown-content menu bg-base-200 rounded-box z-50 w-52 p-1 shadow-lg border border-base-300">
                                <li><a class="text-xs" on:click=move |_| set_show_image_gallery.set(true)>
                                    <span class="icon-[lucide--image] text-sm"></span>"Images"
                                </a></li>
                                <li><a class="text-xs" on:click=move |_| set_show_bib_modal.set(true)>
                                    <span class="icon-[lucide--book-open] text-sm"></span>"Bibliography"
                                </a></li>
                            </ul>
                        </div>

                        <div class="dropdown dropdown-hover">
                            <div tabindex="0" role="button" class="btn btn-ghost btn-xs px-2 text-xs font-normal">"View"</div>
                            <ul tabindex="0" class="dropdown-content menu bg-base-200 rounded-box z-50 w-52 p-1 shadow-lg border border-base-300">
                                <li><a class="text-xs" on:click=move |_| app_state.sidebar_visible.update(|v| *v = !*v)>
                                    <span class="icon-[lucide--panel-left] text-sm"></span>"Toggle Sidebar"
                                </a></li>
                                <li><a class="text-xs" on:click={
                                    let app_state = app_state_menu.clone();
                                    move |_| {
                                        app_state.home_tab.set(crate::state::app_state::HomeTab::Settings);
                                        app_state.current_project.set(None);
                                    }
                                }>
                                    <span class="icon-[lucide--settings] text-sm"></span>"Settings"
                                </a></li>
                            </ul>
                        </div>

                    </Show>

                    <Show when=move || is_compiling.get()>
                        <span class="loading loading-spinner loading-xs text-info ml-1"></span>
                    </Show>
                </div>

                // Right side
                <div class="flex items-center gap-1">
                    // Autosave indicator
                    <Show when=move || has_project() && !app_state.autosave_enabled.get()>
                        <span class="text-[10px] text-warning opacity-60">"autosave off"</span>
                    </Show>
                    <button class="btn btn-ghost btn-xs" title="Settings"
                        on:click=move |_| {
                            app_state.home_tab.set(crate::state::app_state::HomeTab::Settings);
                            app_state.current_project.set(None);
                        }>
                        <span class="icon-[lucide--settings] text-sm"></span>
                    </button>
                </div>
            </header>

            // Main content: editor / home (home includes settings + packages)
            <Show when=show_editor>
                <main
                    class="flex-1 flex overflow-hidden relative min-h-0"
                    on:mousemove=move |ev| {
                        if is_resizing.get() {
                            if let Some(window) = web_sys::window() {
                                let sw = if app_state.sidebar_visible.get_untracked() { app_state.sidebar_width.get_untracked() } else { 0.0 };
                                let w = window.inner_width().unwrap().as_f64().unwrap() - sw;
                                let x = ev.client_x() as f64 - sw;
                                set_editor_width.set(((x / w) * 100.0).clamp(20.0, 80.0));
                            }
                        }
                    }
                    on:mouseup=move |_| set_is_resizing.set(false)
                >
                    <FileSidebar />
                    <div class="overflow-hidden flex flex-col"
                        style:flex=move || format!("0 0 {}%", editor_width.get())>
                        <EditorTabs />
                        <Editor source=source set_source=set_source
                            textarea_ref=textarea_ref insert_at_cursor=insert_at_cursor.clone()
                            on_save=on_save.clone() on_compile=on_compile.clone()
                            set_cursor_ratio=set_cursor_ratio />
                    </div>
                    <div class="w-1 bg-base-300 hover:bg-primary cursor-col-resize transition-colors relative group"
                        on:mousedown=move |ev| { ev.prevent_default(); set_is_resizing.set(true); }>
                        <div class="absolute inset-y-0 -left-1 -right-1 group-hover:bg-primary/20"></div>
                    </div>
                    <div class="flex-1 min-h-0">
                        <Preview output=output error=error
                            pdf_blob_url=pdf_blob_url cursor_ratio=cursor_ratio />
                    </div>
                </main>
            </Show>

            <Show when=show_home>
                <HomePage set_source=set_source set_bib_content=set_bib_content />
            </Show>

            // Bibliography modal
            <Show when=move || show_bib_modal.get()>
                <div class="modal modal-open">
                    <div class="modal-box max-w-4xl">
                        <h3 class="font-bold text-lg flex items-center gap-2">
                            <span class="icon-[lucide--book-open] text-xl"></span>
                            "Bibliography (YAML)"
                        </h3>
                        <div class="form-control mt-2">
                            <textarea
                                class="textarea textarea-bordered h-96 font-mono text-sm"
                                prop:value=move || bib_content.get()
                                on:input=move |ev| set_bib_content.set(event_target_value(&ev))
                            />
                        </div>
                        <div class="modal-action">
                            <button class="btn btn-primary btn-sm gap-2"
                                on:click={
                                    let app_state = app_state_bib.clone();
                                    move |_| {
                                        let bib = bib_content.get();
                                        app_state.file_contents.update(|map| {
                                            map.insert("refs.yml".to_string(), bib.clone());
                                        });
                                        if let Some(project) = app_state.current_project.get_untracked() {
                                            let storage = app_state.storage.clone();
                                            spawn_local(async move {
                                                let _ = storage.write_file(&project.id, "refs.yml",
                                                    &crate::models::FileContent::Text(bib)).await;
                                            });
                                        }
                                        app_state.project_files.update(|files| {
                                            if !files.contains(&"refs.yml".to_string()) {
                                                files.push("refs.yml".to_string());
                                            }
                                        });
                                        set_show_bib_modal.set(false);
                                    }
                                }
                            >
                                <span class="icon-[lucide--save] text-sm"></span>"Save"
                            </button>
                            <button class="btn btn-ghost btn-sm" on:click=move |_| set_show_bib_modal.set(false)>"Cancel"</button>
                        </div>
                    </div>
                </div>
            </Show>

            <ImageGalleryDrawer show=show_image_gallery set_show=set_show_image_gallery
                set_image_cache=set_legacy_image_cache />
            <ProjectSwitcher set_source=set_source set_bib_content=set_bib_content />
        </div>
    }
}

fn download_blob(content: &str, mime: &str, filename: &str) {
    if content.is_empty() { return; }
    if let Some(window) = web_sys::window() {
        if let Some(document) = window.document() {
            let parts = js_sys::Array::new();
            parts.push(&wasm_bindgen::JsValue::from_str(content));
            let opts = web_sys::BlobPropertyBag::new();
            opts.set_type(mime);
            if let Ok(blob) = web_sys::Blob::new_with_str_sequence_and_options(&parts, &opts) {
                if let Ok(url) = web_sys::Url::create_object_url_with_blob(&blob) {
                    if let Ok(link) = document.create_element("a") {
                        let link = link.dyn_into::<web_sys::HtmlAnchorElement>().unwrap();
                        link.set_href(&url);
                        link.set_download(filename);
                        link.click();
                        let _ = web_sys::Url::revoke_object_url(&url);
                    }
                }
            }
        }
    }
}

/// Scan the main source and all project files for package imports.
/// Extracts `@namespace/name:version` from `#import` statements.
/// Filters out file imports (no `@` prefix) and already-installed packages.
fn scan_all_missing_packages(
    main_source: &str,
    file_contents: &HashMap<String, String>,
    cache: &crate::packages::PackageCache,
) -> Vec<crate::packages::PkgSpec> {
    let mut missing = Vec::new();

    // Scan main source
    extract_package_imports(main_source, cache, &mut missing);

    // Scan all other project .typ files
    for (path, content) in file_contents {
        if path.ends_with(".typ") {
            extract_package_imports(content, cache, &mut missing);
        }
    }

    missing
}

/// Extract `@namespace/name:version` package specs from source text.
/// Ignores file imports like `#import "file.typ"`.
fn extract_package_imports(
    source: &str,
    cache: &crate::packages::PackageCache,
    out: &mut Vec<crate::packages::PkgSpec>,
) {
    // Find all @namespace/name:version patterns anywhere in the text.
    // These appear in: #import "@preview/pkg:ver", #import "@preview/pkg:ver": items
    // We look for the pattern `"@` followed by `namespace/name:major.minor.patch"`
    let mut search = source;
    while let Some(at_pos) = search.find("\"@") {
        let rest = &search[at_pos + 1..]; // skip the opening quote, start at @
        // Find the closing quote
        if let Some(quote_end) = rest.find('"') {
            let spec_str = &rest[..quote_end]; // e.g. @preview/cetz:0.3.0
            // Only process if it looks like a package (has @ and namespace/name:version)
            if spec_str.starts_with('@') && spec_str.contains('/') {
                if let Some(spec) = crate::packages::PkgSpec::parse(spec_str) {
                    if !cache.has_package(&spec) && !out.iter().any(|m| m.to_string() == spec.to_string()) {
                        out.push(spec);
                    }
                }
            }
        }
        // Move past this match to find more
        search = &search[at_pos + 2..];
    }
}

fn download_bytes(bytes: &[u8], mime: &str, filename: &str) {
    if let Some(window) = web_sys::window() {
        if let Some(document) = window.document() {
            let uint8 = js_sys::Uint8Array::from(bytes);
            let parts = js_sys::Array::new();
            parts.push(&uint8);
            let opts = web_sys::BlobPropertyBag::new();
            opts.set_type(mime);
            if let Ok(blob) = web_sys::Blob::new_with_u8_array_sequence_and_options(&parts, &opts) {
                if let Ok(url) = web_sys::Url::create_object_url_with_blob(&blob) {
                    if let Ok(link) = document.create_element("a") {
                        let link = link.dyn_into::<web_sys::HtmlAnchorElement>().unwrap();
                        link.set_href(&url);
                        link.set_download(filename);
                        link.click();
                        let _ = web_sys::Url::revoke_object_url(&url);
                    }
                }
            }
        }
    }
}
