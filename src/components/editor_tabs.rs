use leptos::prelude::*;
use crate::state::AppState;

/// Tab bar showing open files above the editor
#[component]
pub fn EditorTabs() -> impl IntoView {
    let app_state = use_context::<AppState>().expect("AppState not provided");

    view! {
        <div class="flex items-center bg-base-200 border-b border-base-300 min-h-8 overflow-x-auto">
            {move || {
                let open = app_state.open_files.get();
                let active = app_state.active_file.get();
                let modified = app_state.modified_files.get();

                open.into_iter().map(|path| {
                    let is_active = active.as_deref() == Some(path.as_str());
                    let is_modified = modified.contains(&path);
                    let path_click = path.clone();
                    let path_close = path.clone();
                    let display_name = path.rsplit('/').next().unwrap_or(&path).to_string();

                    view! {
                        <div
                            class=format!(
                                "flex items-center gap-1 px-3 py-1 text-sm cursor-pointer border-r border-base-300 whitespace-nowrap {}",
                                if is_active { "bg-base-100 text-base-content" } else { "bg-base-200 text-base-content/60 hover:bg-base-300" }
                            )
                            on:click={
                                let app_state = app_state.clone();
                                move |_| {
                                    app_state.active_file.set(Some(path_click.clone()));
                                }
                            }
                        >
                            <span class="icon-[lucide--file-text] text-xs"></span>
                            <span>{display_name}</span>
                            {is_modified.then(|| view! {
                                <span class="w-2 h-2 rounded-full bg-warning ml-1"></span>
                            })}
                            <button
                                class="btn btn-ghost btn-xs ml-1 opacity-60 hover:opacity-100"
                                on:click={
                                    let app_state = app_state.clone();
                                    move |ev: leptos::ev::MouseEvent| {
                                        ev.stop_propagation();
                                        app_state.close_file(&path_close);
                                    }
                                }
                            >
                                <span class="icon-[lucide--x] text-xs"></span>
                            </button>
                        </div>
                    }
                }).collect::<Vec<_>>()
            }}
        </div>
    }
}
