use leptos::prelude::*;
use leptos::html::Textarea;
use leptos::task::spawn_local;
use gloo_timers::future::sleep;
use std::time::Duration;
use crate::state::AppState;
use crate::utils::highlight_typst;
use wasm_bindgen::JsCast;
use std::sync::Arc;

type InsertFn = Arc<dyn Fn(&str, Option<&str>) + Send + Sync>;

#[component]
pub fn Editor(
    source: ReadSignal<String>,
    set_source: WriteSignal<String>,
    textarea_ref: NodeRef<Textarea>,
    insert_at_cursor: InsertFn,
    #[prop(optional)] on_save: Option<Arc<dyn Fn() + Send + Sync>>,
    #[prop(optional)] on_compile: Option<Arc<dyn Fn() + Send + Sync>>,
    #[prop(optional)] set_cursor_ratio: Option<WriteSignal<f64>>,
) -> impl IntoView {
    let app_state = use_context::<AppState>().expect("AppState not provided");

    // Debounced syntax highlighting — avoids re-parsing on every keystroke
    let (highlighted_html, set_highlighted_html) = signal(String::new());
    let (hl_debounce_id, set_hl_debounce_id) = signal(0u32);
    {
        Effect::new(move |_| {
            let src = source.get();
            let current_id = hl_debounce_id.get_untracked() + 1;
            set_hl_debounce_id.set(current_id);
            spawn_local(async move {
                sleep(Duration::from_millis(150)).await;
                if current_id == hl_debounce_id.get_untracked() {
                    // For very large files, skip highlighting to stay responsive
                    if src.len() > 50_000 {
                        // Simple escaped text fallback for huge files
                        let escaped = src.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
                        set_highlighted_html.set(format!("<pre class=\"typst-highlighted\"><code>{}</code></pre>", escaped));
                    } else {
                        set_highlighted_html.set(highlight_typst(&src));
                    }
                }
            });
        });
    }

    // Cursor position: (line, col)
    let (cursor_line, set_cursor_line) = signal(1u32);
    let (cursor_col, set_cursor_col) = signal(1u32);

    let update_cursor_pos = move || {
        if let Some(textarea) = textarea_ref.get() {
            let pos = textarea.selection_start().unwrap_or(None).unwrap_or(0) as usize;
            let text = source.get_untracked();
            let len = text.len();
            let before = &text[..pos.min(len)];
            let line = before.chars().filter(|&c| c == '\n').count() + 1;
            let col = before.rsplit('\n').next().map(|s| s.len()).unwrap_or(pos) + 1;
            set_cursor_line.set(line as u32);
            set_cursor_col.set(col as u32);
            // Emit cursor ratio (0.0 - 1.0)
            if let Some(set_ratio) = set_cursor_ratio {
                let ratio = if len > 0 { pos as f64 / len as f64 } else { 0.0 };
                set_ratio.set(ratio);
            }
        }
    };

    let sync_scroll = move |_| {
        if let Some(textarea) = textarea_ref.get() {
            let scroll_top = textarea.scroll_top();
            let scroll_left = textarea.scroll_left();
            if let Some(window) = web_sys::window() {
                if let Some(document) = window.document() {
                    if let Some(overlay) = document.query_selector(".syntax-overlay").ok().flatten() {
                        if let Some(el) = overlay.dyn_ref::<web_sys::HtmlElement>() {
                            el.set_scroll_top(scroll_top);
                            el.set_scroll_left(scroll_left);
                        }
                    }
                    if let Some(gutter) = document.query_selector(".line-numbers").ok().flatten() {
                        if let Some(el) = gutter.dyn_ref::<web_sys::HtmlElement>() {
                            el.set_scroll_top(scroll_top);
                        }
                    }
                }
            }
        }
    };

    let line_count = Memo::new(move |_| {
        let src = source.get();
        let count = src.lines().count().max(1);
        if src.ends_with('\n') { count + 1 } else { count }
    });

    let font_size = move || app_state.editor_font_size.get();

    view! {
        <div class="flex-1 flex flex-col overflow-hidden border-r border-base-300">
            // Toolbar
            <div class="flex items-center gap-1 px-2 py-0.5 bg-base-200 border-b border-base-300 min-h-7">
                {
                    let on_save = on_save.clone();
                    move || {
                        on_save.clone().map(|save| {
                            view! {
                                <button class="btn btn-ghost btn-xs" title="Save (Ctrl+S)"
                                    on:click=move |_| save()>
                                    <span class="icon-[lucide--save] text-xs"></span>
                                </button>
                            }
                        })
                    }
                }
                {
                    let on_compile = on_compile.clone();
                    move || {
                        on_compile.clone().map(|compile| {
                            view! {
                                <button class="btn btn-ghost btn-xs" title="Compile (Ctrl+Enter)"
                                    on:click=move |_| compile()>
                                    <span class="icon-[lucide--play] text-xs text-success"></span>
                                </button>
                            }
                        })
                    }
                }
                <div class="divider divider-horizontal mx-0 h-4"></div>
                <div class="join join-horizontal">
                    <button class="btn btn-xs join-item" title="Bold"
                        on:click={ let i = insert_at_cursor.clone(); move |_| i("*text*", Some("text")) }>
                        <span class="icon-[lucide--bold] text-xs"></span>
                    </button>
                    <button class="btn btn-xs join-item" title="Italic"
                        on:click={ let i = insert_at_cursor.clone(); move |_| i("_text_", Some("text")) }>
                        <span class="icon-[lucide--italic] text-xs"></span>
                    </button>
                    <button class="btn btn-xs join-item" title="Code"
                        on:click={ let i = insert_at_cursor.clone(); move |_| i("`code`", Some("code")) }>
                        <span class="icon-[lucide--code] text-xs"></span>
                    </button>
                </div>
                <div class="join join-horizontal">
                    <button class="btn btn-xs join-item" title="Heading"
                        on:click={ let i = insert_at_cursor.clone(); move |_| i("= Heading\n", Some("Heading")) }>
                        <span class="icon-[lucide--heading] text-xs"></span>
                    </button>
                    <button class="btn btn-xs join-item" title="List"
                        on:click={ let i = insert_at_cursor.clone(); move |_| i("- Item\n", Some("Item")) }>
                        <span class="icon-[lucide--list] text-xs"></span>
                    </button>
                    <button class="btn btn-xs join-item" title="Math"
                        on:click={ let i = insert_at_cursor.clone(); move |_| i("$ formula $", Some("formula")) }>
                        <span class="icon-[lucide--square-function] text-xs"></span>
                    </button>
                </div>
                <div class="join join-horizontal">
                    <button class="btn btn-xs join-item" title="Figure"
                        on:click={ let i = insert_at_cursor.clone(); move |_| i(
                            "#figure(\n  rect(width: 80%, height: 120pt, fill: rgb(\"#e0e0e0\")),\n  caption: [Your caption here],\n)\n",
                            Some("Your caption here"))
                        }>
                        <span class="icon-[lucide--image] text-xs"></span>
                    </button>
                    <button class="btn btn-xs join-item" title="Table"
                        on:click={ let i = insert_at_cursor.clone(); move |_| i(
                            "#table(\n  columns: 2,\n  [Header 1], [Header 2],\n  [Row 1], [Data],\n)\n",
                            Some("Header 1"))
                        }>
                        <span class="icon-[lucide--table] text-xs"></span>
                    </button>
                    <button class="btn btn-xs join-item" title="Citation"
                        on:click={ let i = insert_at_cursor.clone(); move |_| i("@citation", Some("citation")) }>
                        <span class="icon-[lucide--quote] text-xs"></span>
                    </button>
                    <button class="btn btn-xs join-item" title="Reference"
                        on:click={ let i = insert_at_cursor.clone(); move |_| i("@label", Some("label")) }>
                        <span class="icon-[lucide--link] text-xs"></span>
                    </button>
                </div>
            </div>

            // Editor with line numbers (dynamic font size)
            <div class="flex-1 min-h-0 flex bg-base-100 overflow-hidden">
                // Line number gutter
                <div
                    class="line-numbers flex-shrink-0 overflow-hidden select-none text-right pr-2 pl-2 pt-2 border-r border-base-300/50 bg-base-200/30"
                    style:font-size=move || format!("{}px", font_size())
                    style:line-height="1.6"
                >
                    {move || {
                        let count = line_count();
                        (1..=count).map(|n| {
                            view! { <div class="line-number">{n}</div> }
                        }).collect::<Vec<_>>()
                    }}
                </div>

                // Editor container
                <div class="flex-1 relative overflow-hidden">
                    <div class="editor-container h-full">
                        <div
                            class="syntax-overlay"
                            style:font-size=move || format!("{}px", font_size())
                            style:line-height="1.6"
                            inner_html=move || highlighted_html.get()
                        />
                        <textarea
                            node_ref=textarea_ref
                            class="typst-editor"
                            style:font-size=move || format!("{}px", font_size())
                            style:line-height="1.6"
                            prop:value=move || source.get()
                            on:input=move |ev| {
                                set_source.set(event_target_value(&ev));
                                update_cursor_pos();
                            }
                            on:scroll=sync_scroll
                            on:click=move |_| update_cursor_pos()
                            on:keyup=move |_| update_cursor_pos()
                            placeholder="Write Typst markup here..."
                            spellcheck="false"
                        />
                    </div>
                </div>
            </div>

            // Status bar with cursor position
            <div class="flex items-center px-3 h-5 min-h-5 bg-base-200 border-t border-base-300 text-[10px] text-base-content/50 gap-3 select-none">
                <span>{move || format!("Ln {}, Col {}", cursor_line.get(), cursor_col.get())}</span>
                <span class="opacity-50">{move || {
                    let src = source.get();
                    let lines = src.lines().count();
                    let chars = src.len();
                    format!("{} lines, {} chars", lines, chars)
                }}</span>
                <span class="ml-auto">"UTF-8"</span>
                <span>"Typst"</span>
            </div>
        </div>
    }
}
