use leptos::prelude::*;
use crate::state::AppState;
use crate::state::app_state::{ThemeMode, save_string_setting};
use crate::storage::backend::{StorageBackend, save_backend_choice, load_backend_choice};

/// Full-page settings view
#[component]
pub fn SettingsPage() -> impl IntoView {
    let app_state = use_context::<AppState>().expect("AppState not provided");

    // Local state mirrors for settings (so we can cancel)
    let (selected_backend, set_selected_backend) = signal(load_backend_choice());
    let (server_url, set_server_url) = signal(
        load_setting("server_api_url").unwrap_or_else(|| "http://localhost:3001/api".to_string())
    );
    let (server_token, set_server_token) = signal(
        load_setting("server_api_token").unwrap_or_default()
    );

    view! {
        <div>
                <h1 class="text-2xl font-bold mb-6">"Settings"</h1>

                // --- Editor Section ---
                <section class="mb-8">
                    <h2 class="text-sm font-semibold uppercase tracking-wider text-base-content/50 mb-3">"Editor"</h2>
                    <div class="space-y-1 bg-base-200 rounded-xl p-1">
                        // Autosave toggle
                        <SettingRow
                            icon="icon-[lucide--save]"
                            title="Autosave"
                            description="Automatically save files as you type"
                        >
                            <input
                                type="checkbox"
                                class="toggle toggle-sm toggle-primary"
                                checked=move || app_state.autosave_enabled.get()
                                on:change=move |_| {
                                    app_state.autosave_enabled.update(|v| *v = !*v);
                                    save_string_setting("autosave_enabled",
                                        if app_state.autosave_enabled.get_untracked() { "true" } else { "false" });
                                }
                            />
                        </SettingRow>

                        // Font size
                        <SettingRow
                            icon="icon-[lucide--type]"
                            title="Font Size"
                            description="Editor text size in pixels"
                        >
                            <div class="flex items-center gap-2">
                                <button class="btn btn-ghost btn-xs"
                                    on:click=move |_| {
                                        app_state.editor_font_size.update(|s| *s = (*s).saturating_sub(1).max(8));
                                        save_string_setting("editor_font_size", &app_state.editor_font_size.get_untracked().to_string());
                                    }>
                                    <span class="icon-[lucide--minus] text-xs"></span>
                                </button>
                                <span class="font-mono text-sm min-w-8 text-center">{move || format!("{}px", app_state.editor_font_size.get())}</span>
                                <button class="btn btn-ghost btn-xs"
                                    on:click=move |_| {
                                        app_state.editor_font_size.update(|s| *s = (*s + 1).min(32));
                                        save_string_setting("editor_font_size", &app_state.editor_font_size.get_untracked().to_string());
                                    }>
                                    <span class="icon-[lucide--plus] text-xs"></span>
                                </button>
                            </div>
                        </SettingRow>
                    </div>
                </section>

                // --- Appearance Section ---
                <section class="mb-8">
                    <h2 class="text-sm font-semibold uppercase tracking-wider text-base-content/50 mb-3">"Appearance"</h2>
                    <div class="space-y-1 bg-base-200 rounded-xl p-1">
                        // Theme mode
                        <SettingRow
                            icon="icon-[lucide--palette]"
                            title="Theme"
                            description="Choose between dark, light, or system theme"
                        >
                            <div class="join">
                                <button
                                    class=move || format!("btn btn-xs join-item {}",
                                        if app_state.theme_mode.get() == ThemeMode::Dark { "btn-primary" } else { "btn-ghost" })
                                    on:click=move |_| {
                                        app_state.theme_mode.set(ThemeMode::Dark);
                                        save_string_setting("theme_mode", "dark");
                                    }
                                >
                                    <span class="icon-[lucide--moon] text-xs"></span>
                                    "Dark"
                                </button>
                                <button
                                    class=move || format!("btn btn-xs join-item {}",
                                        if app_state.theme_mode.get() == ThemeMode::Light { "btn-primary" } else { "btn-ghost" })
                                    on:click=move |_| {
                                        app_state.theme_mode.set(ThemeMode::Light);
                                        save_string_setting("theme_mode", "light");
                                    }
                                >
                                    <span class="icon-[lucide--sun] text-xs"></span>
                                    "Light"
                                </button>
                                <button
                                    class=move || format!("btn btn-xs join-item {}",
                                        if app_state.theme_mode.get() == ThemeMode::System { "btn-primary" } else { "btn-ghost" })
                                    on:click=move |_| {
                                        app_state.theme_mode.set(ThemeMode::System);
                                        save_string_setting("theme_mode", "system");
                                    }
                                >
                                    <span class="icon-[lucide--monitor] text-xs"></span>
                                    "System"
                                </button>
                            </div>
                        </SettingRow>
                    </div>
                </section>

                // --- Storage Section ---
                <section class="mb-8">
                    <h2 class="text-sm font-semibold uppercase tracking-wider text-base-content/50 mb-3">"Storage"</h2>
                    <div class="space-y-1 bg-base-200 rounded-xl p-1">
                        // Backend selection
                        <SettingRow
                            icon="icon-[lucide--database]"
                            title="Storage Backend"
                            description="Where your projects are stored"
                        >
                            <div class="join">
                                <button
                                    class=move || format!("btn btn-xs join-item {}",
                                        if selected_backend.get() == StorageBackend::IndexedDb { "btn-primary" } else { "btn-ghost" })
                                    on:click=move |_| set_selected_backend.set(StorageBackend::IndexedDb)
                                >
                                    <span class="icon-[lucide--hard-drive] text-xs"></span>
                                    "Local"
                                </button>
                                <button
                                    class=move || format!("btn btn-xs join-item {}",
                                        if selected_backend.get() == StorageBackend::ServerApi { "btn-primary" } else { "btn-ghost" })
                                    on:click=move |_| set_selected_backend.set(StorageBackend::ServerApi)
                                >
                                    <span class="icon-[lucide--cloud] text-xs"></span>
                                    "Server"
                                </button>
                            </div>
                        </SettingRow>
                    </div>

                    // Server config (shown when server selected)
                    <Show when=move || selected_backend.get() == StorageBackend::ServerApi>
                        <div class="mt-3 bg-base-200 rounded-xl p-4 space-y-3">
                            <div class="form-control">
                                <label class="label py-1">
                                    <span class="label-text text-xs">"Server URL"</span>
                                </label>
                                <input
                                    type="text" class="input input-bordered input-sm text-sm"
                                    placeholder="http://localhost:3001/api"
                                    prop:value=move || server_url.get()
                                    on:input=move |ev| set_server_url.set(event_target_value(&ev))
                                />
                            </div>
                            <div class="form-control">
                                <label class="label py-1">
                                    <span class="label-text text-xs">"Auth Token (optional)"</span>
                                </label>
                                <input
                                    type="password" class="input input-bordered input-sm text-sm"
                                    placeholder="Bearer token..."
                                    prop:value=move || server_token.get()
                                    on:input=move |ev| set_server_token.set(event_target_value(&ev))
                                />
                            </div>
                            <button class="btn btn-primary btn-sm w-full"
                                on:click=move |_| {
                                    let backend = selected_backend.get_untracked();
                                    save_backend_choice(&backend);
                                    save_setting("server_api_url", &server_url.get_untracked());
                                    save_setting("server_api_token", &server_token.get_untracked());
                                    if let Some(window) = web_sys::window() {
                                        let _ = window.location().reload();
                                    }
                                }
                            >
                                "Save & Reload"
                            </button>
                        </div>
                    </Show>
                </section>

                // --- About Section ---
                <section class="mb-8">
                    <h2 class="text-sm font-semibold uppercase tracking-wider text-base-content/50 mb-3">"About"</h2>
                    <div class="bg-base-200 rounded-xl p-4">
                        <div class="flex items-center gap-3">
                            <span class="icon-[lucide--file-type] text-3xl text-primary"></span>
                            <div>
                                <div class="font-bold">"Typst Studio"</div>
                                <div class="text-xs text-base-content/50">"Pure Rust WASM Typst Editor"</div>
                            </div>
                        </div>
                    </div>
                </section>
        </div>
    }
}

/// Reusable setting row with icon, title, description, and a control slot
#[component]
fn SettingRow(
    icon: &'static str,
    title: &'static str,
    description: &'static str,
    children: Children,
) -> impl IntoView {
    view! {
        <div class="flex items-center gap-3 px-4 py-3 rounded-lg hover:bg-base-300/30 transition-colors">
            <span class=format!("{} text-lg text-base-content/50 flex-shrink-0", icon)></span>
            <div class="flex-1 min-w-0">
                <div class="text-sm font-medium">{title}</div>
                <div class="text-xs text-base-content/50">{description}</div>
            </div>
            <div class="flex-shrink-0">
                {children()}
            </div>
        </div>
    }
}

fn load_setting(key: &str) -> Option<String> {
    web_sys::window()
        .and_then(|w| w.local_storage().ok().flatten())
        .and_then(|s| s.get_item(key).ok().flatten())
        .filter(|v| !v.is_empty())
}

fn save_setting(key: &str, value: &str) {
    if let Some(window) = web_sys::window() {
        if let Ok(Some(storage)) = window.local_storage() {
            let _ = storage.set_item(key, value);
        }
    }
}
