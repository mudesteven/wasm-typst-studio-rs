use leptos::prelude::*;
use leptos::task::spawn_local;
use base64::Engine as _;
use crate::state::AppState;
use crate::models::ProjectMetadata;

/// Project switcher — rendered as a dropdown anchored to the project button in navbar.
/// The dropdown is shown/hidden via app_state.show_project_manager signal.
#[component]
pub fn ProjectSwitcher(
    set_source: WriteSignal<String>,
    set_bib_content: WriteSignal<String>,
) -> impl IntoView {
    let app_state = use_context::<AppState>().expect("AppState not provided");
    let show = app_state.show_project_manager;

    let (projects, set_projects) = signal(Vec::<ProjectMetadata>::new());

    {
        let storage = app_state.storage.clone();
        Effect::new(move |_| {
            if show.get() {
                let storage = storage.clone();
                spawn_local(async move {
                    match storage.list_projects().await {
                        Ok(list) => set_projects.set(list),
                        Err(e) => log::error!("Failed to list projects: {}", e),
                    }
                });
            }
        });
    }

    view! {
        // Backdrop to close dropdown
        <Show when=move || show.get()>
            <div class="fixed inset-0 z-40" on:click=move |_| app_state.show_project_manager.set(false)></div>
            <div class="fixed top-9 left-32 z-50 bg-base-200 border border-base-300 rounded-lg shadow-xl w-64 max-h-80 overflow-hidden flex flex-col">
                // Header
                <div class="px-3 py-2 border-b border-base-300 flex items-center justify-between">
                    <span class="text-xs font-semibold text-base-content/50 uppercase">"Projects"</span>
                    <button class="btn btn-ghost btn-xs" title="Home"
                        on:click=move |_| {
                            app_state.current_project.set(None);
                            app_state.show_project_manager.set(false);
                        }>
                        <span class="icon-[lucide--home] text-xs"></span>
                    </button>
                </div>

                // Project list
                <div class="flex-1 overflow-y-auto py-1">
                    <For
                        each=move || projects.get()
                        key=|p| p.id.clone()
                        children={
                            let app_state = app_state.clone();
                            move |project| {
                                let pid = project.id.clone();
                                let name = project.name.clone();
                                let file_count = project.file_count;
                                let current_id = app_state.current_project.get().map(|p| p.id.clone());
                                let is_current = current_id.as_deref() == Some(pid.as_str());

                                let switch = {
                                    let storage = app_state.storage.clone();
                                    let app_state = app_state.clone();
                                    let pid = pid.clone();
                                    move |_| {
                                        let storage = storage.clone();
                                        let app_state = app_state.clone();
                                        let pid = pid.clone();
                                        spawn_local(async move {
                                            load_project_into_state(&app_state, &**storage, &pid).await;
                                            sync_source_from_state(&app_state, set_source, set_bib_content);
                                            app_state.show_project_manager.set(false);
                                        });
                                    }
                                };

                                view! {
                                    <div
                                        class=format!(
                                            "flex items-center gap-2 px-3 py-1.5 cursor-pointer hover:bg-base-300 transition-colors text-xs {}",
                                            if is_current { "bg-primary/10 text-primary" } else { "" }
                                        )
                                        on:click=switch
                                    >
                                        <span class="icon-[lucide--folder] text-sm flex-shrink-0"></span>
                                        <div class="flex-1 min-w-0">
                                            <div class="truncate font-medium">{name}</div>
                                            <div class="text-[10px] text-base-content/40">{format!("{} files", file_count)}</div>
                                        </div>
                                        {is_current.then(|| view! {
                                            <span class="icon-[lucide--check] text-xs text-primary"></span>
                                        })}
                                    </div>
                                }
                            }
                        }
                    />
                </div>
            </div>
        </Show>
    }
}

/// Load a project into the app state
pub async fn load_project_into_state(
    app_state: &AppState,
    storage: &dyn crate::storage::ProjectStorage,
    project_id: &str,
) {
    match storage.get_project(project_id).await {
        Ok(project) => {
            AppState::save_last_project_id(&project.id);
            let files = storage.list_files(&project.id).await.unwrap_or_default();

            let mut contents = std::collections::HashMap::new();
            let mut images = std::collections::HashMap::new();
            for path in &files {
                if let Ok(content) = storage.read_file(&project.id, path).await {
                    match content {
                        crate::models::FileContent::Text(text) => {
                            contents.insert(path.clone(), text);
                        }
                        crate::models::FileContent::Binary(bytes) => {
                            if crate::models::is_image_file(path) {
                                let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
                                let data_url = format!("data:application/octet-stream;base64,{}", b64);
                                images.insert(path.clone(), data_url);
                            }
                        }
                    }
                }
            }

            let main = project.main_file.clone();
            app_state.current_project.set(Some(project));
            app_state.project_files.set(files);
            app_state.file_contents.set(contents);
            app_state.image_cache.set(images);
            app_state.modified_files.update(|s| s.clear());
            app_state.open_files.set(vec![main.clone()]);
            app_state.active_file.set(Some(main));
        }
        Err(e) => log::error!("Failed to load project: {}", e),
    }
}

/// Sync the local source/bib signals from AppState after a project switch
pub fn sync_source_from_state(
    app_state: &AppState,
    set_source: WriteSignal<String>,
    set_bib_content: WriteSignal<String>,
) {
    if let Some(main) = app_state.active_file.get_untracked() {
        let contents = app_state.file_contents.get_untracked();
        if let Some(text) = contents.get(&main) {
            set_source.set(text.clone());
        }
    }
    let contents = app_state.file_contents.get_untracked();
    if let Some(bib) = contents.get("refs.yml") {
        set_bib_content.set(bib.clone());
    } else {
        set_bib_content.set(String::new());
    }
}
