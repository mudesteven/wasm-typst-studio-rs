use leptos::prelude::*;
use leptos::task::spawn_local;
use crate::state::AppState;
use crate::state::app_state::HomeTab;
use crate::models::ProjectMetadata;
use crate::components::project_manager::load_project_into_state;
use crate::components::package_manager::PackageManagerPage;
use crate::components::settings_modal::SettingsPage;

/// Home page with sidebar navigation
#[component]
pub fn HomePage(
    set_source: WriteSignal<String>,
    set_bib_content: WriteSignal<String>,
) -> impl IntoView {
    let app_state = use_context::<AppState>().expect("AppState not provided");

    view! {
        <div class="flex-1 flex overflow-hidden">
            // Sidebar
            <div class="w-56 bg-base-200 border-r border-base-300 flex flex-col">
                // Logo
                <div class="flex items-center gap-2 px-4 py-5">
                    <span class="icon-[lucide--file-type] text-3xl text-primary"></span>
                    <div>
                        <div class="font-bold text-sm">"Typst Studio"</div>
                        <div class="text-[10px] text-base-content/40">"WASM Editor"</div>
                    </div>
                </div>

                // Nav items
                <nav class="flex-1 px-2 space-y-1">
                    <NavItem
                        icon="icon-[lucide--folder-open]"
                        label="Projects"
                        active=Signal::derive(move || app_state.home_tab.get() == HomeTab::Projects)
                        on_click=move |_| app_state.home_tab.set(HomeTab::Projects)
                    />
                    <NavItem
                        icon="icon-[lucide--package]"
                        label="Packages"
                        active=Signal::derive(move || app_state.home_tab.get() == HomeTab::Packages)
                        on_click=move |_| app_state.home_tab.set(HomeTab::Packages)
                    />
                    <NavItem
                        icon="icon-[lucide--settings]"
                        label="Settings"
                        active=Signal::derive(move || app_state.home_tab.get() == HomeTab::Settings)
                        on_click=move |_| app_state.home_tab.set(HomeTab::Settings)
                    />
                </nav>
            </div>

            // Content area
            <div class="flex-1 overflow-auto bg-base-100">
                <div class="max-w-2xl mx-auto px-6 py-8">
                    {move || {
                        match app_state.home_tab.get() {
                            HomeTab::Projects => view! {
                                <ProjectsTab set_source=set_source set_bib_content=set_bib_content />
                            }.into_any(),
                            HomeTab::Packages => view! {
                                <h1 class="text-2xl font-bold mb-6">"Packages"</h1>
                                <PackageManagerPage />
                            }.into_any(),
                            HomeTab::Settings => view! {
                                <SettingsContent />
                            }.into_any(),
                        }
                    }}
                </div>
            </div>
        </div>
    }
}

#[component]
fn NavItem(
    icon: &'static str,
    label: &'static str,
    #[prop(into)] active: Signal<bool>,
    on_click: impl Fn(leptos::ev::MouseEvent) + 'static,
) -> impl IntoView {
    view! {
        <button
            class=move || format!(
                "flex items-center gap-3 w-full px-3 py-2 rounded-lg text-sm transition-colors {}",
                if active.get() { "bg-primary/15 text-primary font-medium" } else { "hover:bg-base-300 text-base-content/70" }
            )
            on:click=on_click
        >
            <span class=format!("{} text-lg", icon)></span>
            {label}
        </button>
    }
}

/// Projects tab content
#[component]
fn ProjectsTab(
    set_source: WriteSignal<String>,
    set_bib_content: WriteSignal<String>,
) -> impl IntoView {
    let app_state = use_context::<AppState>().expect("AppState not provided");
    let (projects, set_projects) = signal(Vec::<ProjectMetadata>::new());
    let (new_name, set_new_name) = signal(String::new());
    let (is_loading, set_is_loading) = signal(true);

    {
        let storage = app_state.storage.clone();
        spawn_local(async move {
            match storage.list_projects().await {
                Ok(list) => set_projects.set(list),
                Err(e) => log::error!("Failed to list projects: {}", e),
            }
            set_is_loading.set(false);
        });
    }

    let on_create = {
        let storage = app_state.storage.clone();
        let app_state = app_state.clone();
        move || {
            let name = new_name.get_untracked();
            if name.trim().is_empty() { return; }
            let storage = storage.clone();
            let app_state = app_state.clone();
            spawn_local(async move {
                log::info!("Creating project: {}", name);
                match storage.create_project(&name).await {
                    Ok(project) => {
                        log::info!("Project created: {} ({})", project.name, project.id);
                        AppState::save_last_project_id(&project.id);
                        let pid = project.id.clone();
                        load_project_into_state(&app_state, &**storage, &pid).await;
                        crate::components::project_manager::sync_source_from_state(&app_state, set_source, set_bib_content);
                        log::info!("Project loaded into state");
                    }
                    Err(e) => log::error!("Failed to create project: {}", e),
                }
                set_new_name.set(String::new());
            });
        }
    };

    let open_project = {
        let storage = app_state.storage.clone();
        let app_state = app_state.clone();
        move |pid: String| {
            let storage = storage.clone();
            let app_state = app_state.clone();
            spawn_local(async move {
                load_project_into_state(&app_state, &**storage, &pid).await;
                crate::components::project_manager::sync_source_from_state(&app_state, set_source, set_bib_content);
            });
        }
    };

    let delete_project = {
        let storage = app_state.storage.clone();
        move |pid: String| {
            let storage = storage.clone();
            spawn_local(async move {
                let _ = storage.delete_project(&pid).await;
                set_projects.update(|list| list.retain(|p| p.id != pid));
            });
        }
    };

    view! {
        <h1 class="text-2xl font-bold mb-6">"Projects"</h1>

        <div class="flex gap-2 mb-6">
            <input type="text" class="input input-bordered flex-1"
                placeholder="New project name..."
                prop:value=move || new_name.get()
                on:input=move |ev| set_new_name.set(event_target_value(&ev))
                on:keydown={
                    let c = on_create.clone();
                    move |ev: leptos::ev::KeyboardEvent| { if ev.key() == "Enter" { c(); } }
                }
            />
            <button class="btn btn-primary gap-2" on:click={
                let c = on_create.clone();
                move |_| c()
            }>
                <span class="icon-[lucide--plus] text-lg"></span>
                "Create"
            </button>
        </div>

        <Show when=move || is_loading.get()>
            <div class="flex justify-center py-12">
                <span class="loading loading-spinner loading-lg"></span>
            </div>
        </Show>

        <Show when=move || !is_loading.get() && projects.get().is_empty()>
            <div class="text-center py-12 text-base-content/40">
                <span class="icon-[lucide--folder-plus] text-5xl block mb-4"></span>
                <p>"No projects yet"</p>
            </div>
        </Show>

        <Show when=move || !is_loading.get() && !projects.get().is_empty()>
            <div class="grid gap-2">
                <For
                    each=move || projects.get()
                    key=|p| p.id.clone()
                    children={
                        let open = open_project.clone();
                        let delete = delete_project.clone();
                        move |project| {
                            let pid = project.id.clone();
                            let pid2 = project.id.clone();
                            let name = project.name.clone();
                            let fc = project.file_count;
                            let open = open.clone();
                            let delete = delete.clone();
                            view! {
                                <div class="flex items-center gap-4 p-4 rounded-xl border border-base-300 hover:border-primary hover:bg-base-200 cursor-pointer transition-all"
                                    on:click={ let o = open.clone(); move |_| o(pid.clone()) }>
                                    <span class="icon-[lucide--folder] text-2xl text-primary"></span>
                                    <div class="flex-1 min-w-0">
                                        <div class="font-semibold truncate">{name}</div>
                                        <div class="text-xs text-base-content/50">{format!("{} files", fc)}</div>
                                    </div>
                                    <button class="btn btn-ghost btn-xs text-error opacity-50 hover:opacity-100"
                                        on:click={
                                            let d = delete.clone();
                                            move |ev: leptos::ev::MouseEvent| { ev.stop_propagation(); d(pid2.clone()); }
                                        }>
                                        <span class="icon-[lucide--trash-2]"></span>
                                    </button>
                                </div>
                            }
                        }
                    }
                />
            </div>
        </Show>
    }
}

/// Settings content (reused from settings page, but inline)
#[component]
fn SettingsContent() -> impl IntoView {
    // Delegate to the full settings page component
    view! { <SettingsPage /> }
}
