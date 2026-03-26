use leptos::prelude::*;
use leptos::task::spawn_local;
use crate::state::AppState;
use crate::packages::registry::{PkgSpec, download_package};

/// Package manager page
#[component]
pub fn PackageManagerPage() -> impl IntoView {
    let app_state = use_context::<AppState>().expect("AppState not provided");

    let (pkg_input, set_pkg_input) = signal(String::new());
    let (is_downloading, set_is_downloading) = signal(false);
    let (status_msg, set_status_msg) = signal(Option::<String>::None);

    let install_package = {
        let app_state = app_state.clone();
        move || {
            let input = pkg_input.get_untracked();
            let input = input.trim().to_string();
            if input.is_empty() { return; }

            let Some(spec) = PkgSpec::parse(&input) else {
                set_status_msg.set(Some("Invalid format. Use @namespace/name:version".to_string()));
                return;
            };

            if app_state.package_cache.has_package(&spec) {
                set_status_msg.set(Some(format!("{} is already installed", spec.to_string())));
                return;
            }

            let cache = app_state.package_cache.clone();
            set_is_downloading.set(true);
            set_status_msg.set(Some(format!("Downloading {}...", spec.to_string())));

            spawn_local(async move {
                match download_package(&spec).await {
                    Ok(files) => {
                        let count = files.len();
                        match cache.store_package(&spec, files).await {
                            Ok(()) => {
                                set_status_msg.set(Some(format!("Installed {} ({} files)", spec.to_string(), count)));
                                set_pkg_input.set(String::new());
                            }
                            Err(e) => set_status_msg.set(Some(format!("Cache error: {}", e))),
                        }
                    }
                    Err(e) => set_status_msg.set(Some(format!("Download failed: {}", e))),
                }
                set_is_downloading.set(false);
            });
        }
    };

    view! {
        <div>
            // Install input
            <div class="flex gap-2 mb-6">
                <input
                    type="text"
                    class="input input-bordered flex-1"
                    placeholder="@preview/package:version"
                    prop:value=move || pkg_input.get()
                    on:input=move |ev| set_pkg_input.set(event_target_value(&ev))
                    on:keydown={
                        let install = install_package.clone();
                        move |ev: leptos::ev::KeyboardEvent| {
                            if ev.key() == "Enter" { install(); }
                        }
                    }
                    disabled=move || is_downloading.get()
                />
                <button
                    class="btn btn-primary gap-2"
                    disabled=move || is_downloading.get()
                    on:click={
                        let install = install_package.clone();
                        move |_| install()
                    }
                >
                    <Show when=move || is_downloading.get()>
                        <span class="loading loading-spinner loading-xs"></span>
                    </Show>
                    <Show when=move || !is_downloading.get()>
                        <span class="icon-[lucide--download] text-lg"></span>
                    </Show>
                    "Install"
                </button>
            </div>

            // Status message
            {move || status_msg.get().map(|msg| {
                let is_error = msg.contains("failed") || msg.contains("error") || msg.contains("Invalid");
                let msg2 = msg.clone();
                view! {
                    <div class=format!("alert mb-4 text-sm {}",
                        if is_error { "alert-error" } else { "alert-success" })>
                        {msg2}
                    </div>
                }
            })}

            // Installed packages
            <h3 class="text-sm font-semibold mb-3 text-base-content/60">"Installed Packages"</h3>
            {move || {
                let packages = app_state.package_cache.list_packages();
                if packages.is_empty() {
                    view! {
                        <div class="text-center py-8 text-base-content/40">
                            <span class="icon-[lucide--package] text-4xl block mb-3"></span>
                            <p class="text-sm">"No packages installed"</p>
                            <p class="text-xs mt-1">"Try: @preview/cetz:0.3.0 or @preview/tablex:0.0.8"</p>
                        </div>
                    }.into_any()
                } else {
                    view! {
                        <div class="space-y-2">
                            {packages.into_iter().map(|spec| {
                                let spec_str = spec.to_string();
                                let spec_display = spec_str.clone();
                                let spec_name = format!("{}/{}", spec.namespace, spec.name);
                                let spec_ver = spec.version.clone();
                                let cache = app_state.package_cache.clone();
                                let spec_del = spec.clone();

                                // Count files
                                let file_count = app_state.package_cache.packages.get_untracked()
                                    .get(&spec_str).map(|f| f.len()).unwrap_or(0);

                                view! {
                                    <div class="flex items-center gap-3 p-3 rounded-lg border border-base-300 hover:bg-base-200 transition-colors">
                                        <span class="icon-[lucide--package] text-xl text-primary"></span>
                                        <div class="flex-1 min-w-0">
                                            <div class="font-medium text-sm truncate">{spec_name}</div>
                                            <div class="text-xs text-base-content/50">{format!("v{} - {} files", spec_ver, file_count)}</div>
                                        </div>
                                        <button
                                            class="btn btn-ghost btn-xs text-error"
                                            title="Uninstall"
                                            on:click=move |_| {
                                                let cache = cache.clone();
                                                let spec = spec_del.clone();
                                                spawn_local(async move {
                                                    let _ = cache.remove_package(&spec).await;
                                                });
                                            }
                                        >
                                            <span class="icon-[lucide--trash-2] text-sm"></span>
                                        </button>
                                    </div>
                                }
                            }).collect::<Vec<_>>()}
                        </div>
                    }.into_any()
                }
            }}
        </div>
    }
}

/// Install missing packages from error message, returns true if any were installed
pub async fn install_missing_packages(error: &str, cache: &crate::packages::PackageCache) -> bool {
    let specs = crate::packages::registry::extract_missing_packages(error);
    if specs.is_empty() { return false; }

    let mut installed = false;
    for spec in &specs {
        if cache.has_package(spec) { continue; }
        log::info!("Auto-installing package: {}", spec.to_string());
        match download_package(spec).await {
            Ok(files) => {
                if cache.store_package(spec, files).await.is_ok() {
                    installed = true;
                    log::info!("Installed: {}", spec.to_string());
                }
            }
            Err(e) => log::error!("Failed to install {}: {}", spec.to_string(), e),
        }
    }
    installed
}
