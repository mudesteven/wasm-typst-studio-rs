use leptos::prelude::*;
use leptos::task::spawn_local;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use crate::state::AppState;
use crate::models::{build_file_tree, FileTreeNode, is_image_file, is_text_file, FileContent};
use std::collections::HashSet;

/// Shared drag/rename state for the file tree
#[derive(Clone)]
struct TreeState {
    dragging: RwSignal<Option<String>>,
    drop_target: RwSignal<Option<String>>,
    /// Path of the file currently being renamed (inline edit)
    renaming: RwSignal<Option<String>>,
    /// The new name being typed
    rename_value: RwSignal<String>,
}

/// Collapsible file manager sidebar
#[component]
pub fn FileSidebar() -> impl IntoView {
    let app_state = use_context::<AppState>().expect("AppState not provided");
    let visible = app_state.sidebar_visible;

    let (new_item_name, set_new_item_name) = signal(String::new());
    let (show_new_input, set_show_new_input) = signal(false);
    let (creating_folder, set_creating_folder) = signal(false);
    let (drag_over_upload, set_drag_over_upload) = signal(false);

    // Tree state (drag + rename)
    let drag_state = TreeState {
        dragging: RwSignal::new(None),
        drop_target: RwSignal::new(None),
        renaming: RwSignal::new(None),
        rename_value: RwSignal::new(String::new()),
    };
    provide_context(drag_state.clone());

    // Track expanded folders
    let (expanded_dirs, set_expanded_dirs) = signal(HashSet::<String>::new());
    {
        let app_state = app_state.clone();
        Effect::new(move |_| {
            let files = app_state.project_files.get();
            let mut dirs = HashSet::new();
            for f in &files {
                let parts: Vec<&str> = f.split('/').collect();
                let mut path = String::new();
                for part in &parts[..parts.len().saturating_sub(1)] {
                    if !path.is_empty() { path.push('/'); }
                    path.push_str(part);
                    dirs.insert(path.clone());
                }
            }
            set_expanded_dirs.set(dirs);
        });
    }

    let create_item = {
        let app_state = app_state.clone();
        move || {
            let name = new_item_name.get_untracked();
            if name.trim().is_empty() { return; }
            let is_folder = creating_folder.get_untracked();
            let project = app_state.current_project.get_untracked();
            if let Some(project) = project {
                let storage = app_state.storage.clone();
                let app_state = app_state.clone();
                let name = name.clone();
                spawn_local(async move {
                    if is_folder {
                        let initial_file = format!("{}/main.typ", name);
                        let content = FileContent::Text(String::new());
                        if let Err(e) = storage.write_file(&project.id, &initial_file, &content).await {
                            log::error!("Failed to create folder: {}", e);
                            return;
                        }
                        app_state.project_files.update(|files| {
                            if !files.contains(&initial_file) { files.push(initial_file.clone()); files.sort(); }
                        });
                        app_state.file_contents.update(|map| { map.insert(initial_file, String::new()); });
                        set_expanded_dirs.update(|dirs| { dirs.insert(name); });
                    } else {
                        let content = FileContent::Text(String::new());
                        if let Err(e) = storage.write_file(&project.id, &name, &content).await {
                            log::error!("Failed to create file: {}", e);
                            return;
                        }
                        app_state.project_files.update(|files| {
                            if !files.contains(&name) { files.push(name.clone()); files.sort(); }
                        });
                        app_state.file_contents.update(|map| { map.insert(name.clone(), String::new()); });
                        app_state.open_file(&name);
                    }
                    set_new_item_name.set(String::new());
                    set_show_new_input.set(false);
                });
            }
        }
    };

    // Handle file upload via drag from OS
    let handle_upload_drop = {
        let app_state = app_state.clone();
        move |ev: web_sys::DragEvent| {
            ev.prevent_default();
            set_drag_over_upload.set(false);

            // Ignore internal drags
            if drag_state.dragging.get_untracked().is_some() { return; }

            let project = app_state.current_project.get_untracked();
            let Some(project) = project else { return };
            let Some(dt) = ev.data_transfer() else { return };
            let Some(files) = dt.files() else { return };

            let storage = app_state.storage.clone();
            let app_state = app_state.clone();
            let pid = project.id.clone();
            spawn_local(async move {
                for i in 0..files.length() {
                    let Some(file) = files.get(i) else { continue };
                    let filename = file.name();
                    if is_text_file(&filename) {
                        if let Some(text) = read_file_as_text(&file).await {
                            let content = FileContent::Text(text.clone());
                            let _ = storage.write_file(&pid, &filename, &content).await;
                            app_state.project_files.update(|f| {
                                if !f.contains(&filename) { f.push(filename.clone()); f.sort(); }
                            });
                            app_state.file_contents.update(|map| { map.insert(filename.clone(), text); });
                            app_state.open_file(&filename);
                        }
                    } else {
                        if let Some(bytes) = read_file_as_bytes(&file).await {
                            let content = FileContent::Binary(bytes.clone());
                            let _ = storage.write_file(&pid, &filename, &content).await;
                            app_state.project_files.update(|f| {
                                if !f.contains(&filename) { f.push(filename.clone()); f.sort(); }
                            });
                            if is_image_file(&filename) {
                                let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
                                app_state.image_cache.update(|c| {
                                    c.insert(filename.clone(), format!("data:application/octet-stream;base64,{}", b64));
                                });
                            }
                        }
                    }
                    log::info!("Uploaded: {}", filename);
                }
            });
        }
    };

    // Handle internal file move drop on root (move to root level)
    let handle_root_drop = {
        let app_state = app_state.clone();
        let drag_state = drag_state.clone();
        move |ev: web_sys::DragEvent| {
            ev.prevent_default();
            set_drag_over_upload.set(false);

            let Some(source_path) = drag_state.dragging.get_untracked() else { return };
            drag_state.dragging.set(None);
            drag_state.drop_target.set(None);

            // Move file to root: new_path = filename only
            let filename = source_path.rsplit('/').next().unwrap_or(&source_path);
            if filename == source_path { return; } // Already at root

            let new_path = filename.to_string();
            move_file(&app_state, &source_path, &new_path);
        }
    };

    let (is_resizing_sidebar, set_is_resizing_sidebar) = signal(false);

    view! {
        <div class="flex flex-shrink-0" style:width=move || {
            if visible.get() { format!("{}px", app_state.sidebar_width.get()) } else { "0px".to_string() }
        }>
        <div
            class="bg-base-200 flex flex-col overflow-hidden flex-1"
            style:display=move || if visible.get() { "flex" } else { "none" }
        >
            // Header
            <div class="flex items-center justify-between px-2 py-1 border-b border-base-300">
                <span class="text-xs font-semibold text-base-content/50 uppercase tracking-wider">"Explorer"</span>
                <div class="flex gap-0">
                    <button class="btn btn-ghost btn-xs" title="New file"
                        on:click=move |_| { set_creating_folder.set(false); set_show_new_input.update(|v| *v = !*v); }>
                        <span class="icon-[lucide--file-plus] text-xs"></span>
                    </button>
                    <button class="btn btn-ghost btn-xs" title="New folder"
                        on:click=move |_| { set_creating_folder.set(true); set_show_new_input.update(|v| *v = !*v); }>
                        <span class="icon-[lucide--folder-plus] text-xs"></span>
                    </button>
                    <button class="btn btn-ghost btn-xs" title="Rename active file"
                        on:click={
                            let drag_state = drag_state.clone();
                            move |_| {
                                if let Some(active) = app_state.active_file.get_untracked() {
                                    let filename = active.rsplit('/').next().unwrap_or(&active).to_string();
                                    drag_state.rename_value.set(filename);
                                    drag_state.renaming.set(Some(active));
                                }
                            }
                        }>
                        <span class="icon-[lucide--pencil] text-xs"></span>
                    </button>
                </div>
            </div>

            // New item input
            <Show when=move || show_new_input.get()>
                <div class="px-2 py-1 border-b border-base-300 flex items-center gap-1">
                    <span class=move || if creating_folder.get() {
                        "icon-[lucide--folder-plus] text-xs text-warning"
                    } else {
                        "icon-[lucide--file-plus] text-xs text-info"
                    }></span>
                    <input type="text" class="input input-bordered input-xs flex-1"
                        placeholder=move || if creating_folder.get() { "folder-name" } else { "filename.typ" }
                        prop:value=move || new_item_name.get()
                        on:input=move |ev| set_new_item_name.set(event_target_value(&ev))
                        on:keydown={
                            let create = create_item.clone();
                            move |ev: leptos::ev::KeyboardEvent| {
                                if ev.key() == "Enter" { create(); }
                                else if ev.key() == "Escape" { set_show_new_input.set(false); }
                            }
                        }
                    />
                </div>
            </Show>

            // File tree (drop zone for both uploads and internal moves)
            <div
                class=move || format!(
                    "flex-1 overflow-y-auto py-0.5 text-xs transition-colors {}",
                    if drag_over_upload.get() && drag_state.dragging.get().is_none() {
                        "bg-primary/10 border-2 border-dashed border-primary/30"
                    } else { "" }
                )
                on:dragover=move |ev: web_sys::DragEvent| {
                    ev.prevent_default();
                    if drag_state.dragging.get_untracked().is_none() {
                        set_drag_over_upload.set(true);
                    }
                    // Set drop target to root for internal drags
                    if drag_state.dragging.get_untracked().is_some() {
                        drag_state.drop_target.set(Some("".to_string()));
                    }
                }
                on:dragleave=move |_: web_sys::DragEvent| {
                    set_drag_over_upload.set(false);
                    drag_state.drop_target.set(None);
                }
                on:drop={
                    let handle_upload = handle_upload_drop.clone();
                    let handle_root = handle_root_drop.clone();
                    move |ev: web_sys::DragEvent| {
                        if drag_state.dragging.get_untracked().is_some() {
                            handle_root(ev);
                        } else {
                            handle_upload(ev);
                        }
                    }
                }
            >
                <Show when=move || drag_over_upload.get() && drag_state.dragging.get().is_none()>
                    <div class="flex items-center justify-center py-4 text-primary/60">
                        <span class="icon-[lucide--upload] text-lg mr-2"></span>
                        <span class="text-xs">"Drop files here"</span>
                    </div>
                </Show>

                {move || {
                    let files = app_state.project_files.get();
                    let tree = build_file_tree(&files);
                    view! { <TreeNodes nodes=tree depth=0 expanded_dirs=expanded_dirs set_expanded_dirs=set_expanded_dirs /> }
                }}
            </div>

            // Project info
            <div class="px-2 py-1 border-t border-base-300 text-[10px] text-base-content/40">
                {move || {
                    let files = app_state.project_files.get();
                    let name = app_state.current_project.get().map(|p| p.name.clone()).unwrap_or_default();
                    let name2 = name.clone();
                    view! {
                        <div class="truncate" title=name>
                            <span class="icon-[lucide--folder] text-[10px] mr-0.5"></span>
                            {name2}
                            <span class="ml-1 opacity-50">{format!("({} files)", files.len())}</span>
                        </div>
                    }
                }}
            </div>
        </div>
        <div class="w-1 cursor-col-resize hover:bg-primary/40 transition-colors flex-shrink-0"
            on:mousedown=move |ev| { ev.prevent_default(); set_is_resizing_sidebar.set(true); }
        ></div>
        </div>

        {
            let app_state = app_state.clone();
            spawn_local(async move {
                use wasm_bindgen::closure::Closure;
                if let Some(window) = web_sys::window() {
                    let mousemove = Closure::wrap(Box::new(move |ev: web_sys::MouseEvent| {
                        if is_resizing_sidebar.get_untracked() {
                            app_state.sidebar_width.set((ev.client_x() as f64).clamp(120.0, 500.0));
                        }
                    }) as Box<dyn FnMut(_)>);
                    let mouseup = Closure::wrap(Box::new(move |_: web_sys::MouseEvent| {
                        set_is_resizing_sidebar.set(false);
                    }) as Box<dyn FnMut(_)>);
                    let _ = window.add_event_listener_with_callback("mousemove", mousemove.as_ref().unchecked_ref());
                    let _ = window.add_event_listener_with_callback("mouseup", mouseup.as_ref().unchecked_ref());
                    mousemove.forget();
                    mouseup.forget();
                }
            });
        }
    }
}

/// Move a file from old_path to new_path in storage and update state
fn move_file(app_state: &AppState, old_path: &str, new_path: &str) {
    if old_path == new_path { return; }

    let project = app_state.current_project.get_untracked();
    let Some(project) = project else { return };
    let storage = app_state.storage.clone();
    let app_state = app_state.clone();
    let old = old_path.to_string();
    let new = new_path.to_string();

    spawn_local(async move {
        if let Err(e) = storage.rename_file(&project.id, &old, &new).await {
            log::error!("Failed to move file: {}", e);
            return;
        }

        // Update project_files
        app_state.project_files.update(|files| {
            if let Some(idx) = files.iter().position(|f| f == &old) {
                files[idx] = new.clone();
            }
            files.sort();
        });

        // Update file_contents
        app_state.file_contents.update(|map| {
            if let Some(content) = map.remove(&old) {
                map.insert(new.clone(), content);
            }
        });

        // Update image_cache
        app_state.image_cache.update(|cache| {
            if let Some(data) = cache.remove(&old) {
                cache.insert(new.clone(), data);
            }
        });

        // Update open files and active file
        app_state.open_files.update(|files| {
            for f in files.iter_mut() {
                if *f == old { *f = new.clone(); }
            }
        });
        if app_state.active_file.get_untracked().as_deref() == Some(&old) {
            app_state.active_file.set(Some(new.clone()));
        }

        log::info!("Moved: {} -> {}", old, new);
    });
}

/// Tree nodes renderer
#[component]
fn TreeNodes(
    nodes: Vec<FileTreeNode>,
    depth: usize,
    expanded_dirs: ReadSignal<HashSet<String>>,
    set_expanded_dirs: WriteSignal<HashSet<String>>,
) -> impl IntoView {
    nodes.into_iter().map(move |node| {
        match node {
            FileTreeNode::File { name, path } => {
                view! { <FileItem name=name path=path depth=depth /> }.into_any()
            }
            FileTreeNode::Directory { name, path, children, .. } => {
                view! {
                    <DirItem name=name path=path children=children depth=depth
                        expanded_dirs=expanded_dirs set_expanded_dirs=set_expanded_dirs />
                }.into_any()
            }
        }
    }).collect::<Vec<_>>()
}

/// Draggable file entry
#[component]
fn FileItem(name: String, path: String, depth: usize) -> impl IntoView {
    let app_state = use_context::<AppState>().expect("AppState not provided");
    let drag_state = use_context::<TreeState>().expect("DragState not provided");
    let padding = depth * 14 + 6;

    let icon_class = if is_image_file(&name) {
        "icon-[lucide--image] text-purple-400"
    } else if name.ends_with(".yml") || name.ends_with(".yaml") || name.ends_with(".bib") {
        "icon-[lucide--book-open] text-orange-400"
    } else if name.ends_with(".typ") {
        "icon-[lucide--file-text] text-blue-400"
    } else {
        "icon-[lucide--file] text-base-content/40"
    };

    let path_click = path.clone();
    let path_delete = path.clone();
    let path_reactive = path.clone();
    let path_drag = path.clone();
    let path_rename = path.clone();
    let path_rename2 = path.clone();
    let app_state_rename = app_state.clone();
    let drag_state_rename = drag_state.clone();

    let on_click = {
        let app_state = app_state.clone();
        move |_| {
            let path = path_click.clone();
            let contents = app_state.file_contents.get_untracked();
            if !contents.contains_key(&path) {
                if let Some(project) = app_state.current_project.get_untracked() {
                    let storage = app_state.storage.clone();
                    let app_state = app_state.clone();
                    let p = path.clone();
                    spawn_local(async move {
                        if let Ok(content) = storage.read_file(&project.id, &p).await {
                            if let Some(text) = content.as_text() {
                                app_state.file_contents.update(|map| { map.insert(p.clone(), text.to_string()); });
                            }
                        }
                        app_state.open_file(&p);
                    });
                }
            } else {
                app_state.open_file(&path);
            }
        }
    };

    let on_delete = {
        let app_state = app_state.clone();
        move |ev: leptos::ev::MouseEvent| {
            ev.stop_propagation();
            let path = path_delete.clone();
            if let Some(project) = app_state.current_project.get_untracked() {
                let storage = app_state.storage.clone();
                let app_state = app_state.clone();
                spawn_local(async move {
                    if let Err(e) = storage.delete_file(&project.id, &path).await {
                        log::error!("Failed to delete: {}", e);
                        return;
                    }
                    app_state.project_files.update(|files| files.retain(|f| f != &path));
                    app_state.file_contents.update(|map| { map.remove(&path); });
                    app_state.close_file(&path);
                });
            }
        }
    };

    view! {
        <div
            draggable="true"
            class=move || {
                let active = app_state.active_file.get();
                let is_active = active.as_deref() == Some(path_reactive.as_str());
                format!(
                    "flex items-center gap-1.5 py-0.5 pr-1 cursor-grab hover:bg-base-300/50 transition-colors group {}",
                    if is_active { "bg-primary/15 text-primary font-medium" } else { "" }
                )
            }
            style:padding-left=format!("{}px", padding)
            style:opacity=move || {
                if drag_state.dragging.get().as_deref() == Some(path_drag.as_str()) { "0.4" } else { "1" }
            }
            on:click=on_click
            on:dragstart=move |ev: web_sys::DragEvent| {
                drag_state.dragging.set(Some(path.clone()));
                if let Some(dt) = ev.data_transfer() {
                    let _ = dt.set_data("text/plain", &path);
                    dt.set_effect_allowed("move");
                }
            }
            on:dragend=move |_: web_sys::DragEvent| {
                drag_state.dragging.set(None);
                drag_state.drop_target.set(None);
            }
        >
            <span class=format!("{} text-[11px] flex-shrink-0", icon_class)></span>
            // Inline rename or normal name
            {move || {
                let is_renaming = drag_state_rename.renaming.get().as_deref() == Some(path_rename.as_str());
                if is_renaming {
                    let app_state = app_state_rename.clone();
                    let drag_state = drag_state_rename.clone();
                    let old_path = path_rename2.clone();
                    let commit_rename = move || {
                        let new_name = drag_state.rename_value.get_untracked();
                        if new_name.trim().is_empty() || new_name == old_path.rsplit('/').next().unwrap_or(&old_path) {
                            drag_state.renaming.set(None);
                            return;
                        }
                        // Compute new full path: keep parent dir, replace filename
                        let new_path = if let Some(idx) = old_path.rfind('/') {
                            format!("{}/{}", &old_path[..idx], new_name)
                        } else {
                            new_name
                        };
                        move_file(&app_state, &old_path, &new_path);
                        drag_state.renaming.set(None);
                    };
                    let commit = commit_rename.clone();
                    view! {
                        <input
                            type="text"
                            class="input input-bordered input-xs flex-1 h-4 min-h-4 text-xs px-1"
                            prop:value=move || drag_state.rename_value.get()
                            on:input=move |ev| drag_state.rename_value.set(event_target_value(&ev))
                            on:keydown=move |ev: leptos::ev::KeyboardEvent| {
                                if ev.key() == "Enter" { commit_rename(); }
                                else if ev.key() == "Escape" { drag_state.renaming.set(None); }
                            }
                            on:blur=move |_| commit()
                            draggable="false"
                        />
                    }.into_any()
                } else {
                    view! {
                        <span class="truncate flex-1"
                            on:dblclick={
                                let drag_state = drag_state_rename.clone();
                                let p = path_rename.clone();
                                move |ev: leptos::ev::MouseEvent| {
                                    ev.stop_propagation();
                                    let filename = p.rsplit('/').next().unwrap_or(&p).to_string();
                                    drag_state.rename_value.set(filename);
                                    drag_state.renaming.set(Some(p.clone()));
                                }
                            }
                        >{name.clone()}</span>
                    }.into_any()
                }
            }}
            <button
                class="btn btn-ghost btn-xs opacity-0 group-hover:opacity-100 text-error h-4 min-h-4 w-4 p-0"
                title="Delete" on:click=on_delete
                draggable="false"
            >
                <span class="icon-[lucide--x] text-[10px]"></span>
            </button>
        </div>
    }
}

/// Droppable directory entry
#[component]
fn DirItem(
    name: String,
    path: String,
    children: Vec<FileTreeNode>,
    depth: usize,
    expanded_dirs: ReadSignal<HashSet<String>>,
    set_expanded_dirs: WriteSignal<HashSet<String>>,
) -> impl IntoView {
    let app_state = use_context::<AppState>().expect("AppState not provided");
    let drag_state = use_context::<TreeState>().expect("DragState not provided");
    let padding = depth * 14 + 6;
    let path_toggle = path.clone();
    let path_reactive = path.clone();
    let path_reactive2 = path.clone();
    let path_drop = path.clone();
    let path_drop_target = path.clone();
    let path_reactive3 = path.clone();
    let path_reactive4 = path.clone();

    let toggle = move |_| {
        let p = path_toggle.clone();
        set_expanded_dirs.update(|dirs| {
            if dirs.contains(&p) { dirs.remove(&p); } else { dirs.insert(p); }
        });
    };

    view! {
        <div>
            <div
                draggable="true"
                class=move || {
                    let is_drop = drag_state.drop_target.get().as_deref() == Some(path_drop_target.as_str());
                    format!(
                        "flex items-center gap-1.5 py-0.5 pr-1 cursor-grab hover:bg-base-300/50 transition-colors text-base-content/70 {}",
                        if is_drop { "bg-primary/20 outline outline-1 outline-primary/50 rounded" } else { "" }
                    )
                }
                style:padding-left=format!("{}px", padding)
                on:click=toggle

                // Drag this folder
                on:dragstart=move |ev: web_sys::DragEvent| {
                    drag_state.dragging.set(Some(path.clone()));
                    if let Some(dt) = ev.data_transfer() {
                        let _ = dt.set_data("text/plain", &path);
                        dt.set_effect_allowed("move");
                    }
                }
                on:dragend=move |_: web_sys::DragEvent| {
                    drag_state.dragging.set(None);
                    drag_state.drop_target.set(None);
                }

                // Accept drops onto this folder
                on:dragover=move |ev: web_sys::DragEvent| {
                    ev.prevent_default();
                    ev.stop_propagation();
                    if drag_state.dragging.get_untracked().is_some() {
                        drag_state.drop_target.set(Some(path_drop.clone()));
                        // Auto-expand folder on hover
                        set_expanded_dirs.update(|dirs| { dirs.insert(path_drop.clone()); });
                    }
                }
                on:dragleave=move |ev: web_sys::DragEvent| {
                    ev.stop_propagation();
                    drag_state.drop_target.set(None);
                }
                on:drop={
                    let app_state = app_state.clone();
                    let drag_state = drag_state.clone();
                    let folder_path = path_reactive3.clone();
                    move |ev: web_sys::DragEvent| {
                        ev.prevent_default();
                        ev.stop_propagation();

                        let Some(source_path) = drag_state.dragging.get_untracked() else { return };
                        drag_state.dragging.set(None);
                        drag_state.drop_target.set(None);

                        // Don't drop folder into itself
                        if source_path == folder_path || source_path.starts_with(&format!("{}/", folder_path)) {
                            return;
                        }

                        // Compute new path: folder/filename
                        let filename = source_path.rsplit('/').next().unwrap_or(&source_path);
                        let new_path = format!("{}/{}", folder_path, filename);

                        move_file(&app_state, &source_path, &new_path);
                    }
                }
            >
                <span class=move || {
                    if expanded_dirs.get().contains(&path_reactive) {
                        "icon-[lucide--chevron-down] text-[10px] flex-shrink-0 opacity-50"
                    } else {
                        "icon-[lucide--chevron-right] text-[10px] flex-shrink-0 opacity-50"
                    }
                }></span>
                <span class=move || {
                    let is_drop = drag_state.drop_target.get().as_deref() == Some(path_reactive2.as_str());
                    if expanded_dirs.get().contains(&path_reactive2) || is_drop {
                        "icon-[lucide--folder-open] text-[11px] text-warning flex-shrink-0"
                    } else {
                        "icon-[lucide--folder] text-[11px] text-warning flex-shrink-0"
                    }
                }></span>
                <span class="truncate font-medium">{name}</span>
            </div>
            {move || {
                if expanded_dirs.get().contains(&path_reactive4) {
                    Some(view! {
                        <TreeNodes nodes=children.clone() depth=depth + 1
                            expanded_dirs=expanded_dirs set_expanded_dirs=set_expanded_dirs />
                    })
                } else { None }
            }}
        </div>
    }
}

// --- File reading helpers ---
use wasm_bindgen_futures::JsFuture;
use base64::Engine as _;

async fn read_file_as_text(file: &web_sys::File) -> Option<String> {
    let reader = web_sys::FileReader::new().ok()?;
    let promise = js_sys::Promise::new(&mut |resolve, reject| {
        let r = reader.clone();
        let ok = Closure::wrap(Box::new(move |_: web_sys::Event| {
            if let Ok(res) = r.result() { let _ = resolve.call1(&JsValue::NULL, &res); }
        }) as Box<dyn FnMut(_)>);
        let err = Closure::wrap(Box::new(move |_: web_sys::Event| {
            let _ = reject.call1(&JsValue::NULL, &JsValue::from_str("read error"));
        }) as Box<dyn FnMut(_)>);
        reader.set_onload(Some(ok.as_ref().unchecked_ref()));
        reader.set_onerror(Some(err.as_ref().unchecked_ref()));
        ok.forget(); err.forget();
        let _ = reader.read_as_text(file);
    });
    JsFuture::from(promise).await.ok()?.as_string()
}

async fn read_file_as_bytes(file: &web_sys::File) -> Option<Vec<u8>> {
    let reader = web_sys::FileReader::new().ok()?;
    let promise = js_sys::Promise::new(&mut |resolve, reject| {
        let r = reader.clone();
        let ok = Closure::wrap(Box::new(move |_: web_sys::Event| {
            if let Ok(res) = r.result() { let _ = resolve.call1(&JsValue::NULL, &res); }
        }) as Box<dyn FnMut(_)>);
        let err = Closure::wrap(Box::new(move |_: web_sys::Event| {
            let _ = reject.call1(&JsValue::NULL, &JsValue::from_str("read error"));
        }) as Box<dyn FnMut(_)>);
        reader.set_onload(Some(ok.as_ref().unchecked_ref()));
        reader.set_onerror(Some(err.as_ref().unchecked_ref()));
        ok.forget(); err.forget();
        let _ = reader.read_as_array_buffer(file);
    });
    let result = JsFuture::from(promise).await.ok()?;
    Some(js_sys::Uint8Array::new(&result).to_vec())
}
