use leptos::prelude::*;
use leptos::either::*;
use leptos::task::spawn_local;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use crate::state::AppState;

/// Preview mode
#[derive(Clone, Copy, PartialEq)]
enum PreviewMode {
    Svg,
    Pdf,
}

#[component]
pub fn Preview(
    output: ReadSignal<String>,
    error: ReadSignal<Option<String>>,
    #[prop(optional)] pdf_blob_url: Option<ReadSignal<Option<String>>>,
    #[prop(optional)] cursor_ratio: Option<ReadSignal<f64>>,
) -> impl IntoView {
    let (mode, set_mode) = signal(PreviewMode::Svg);
    let (zoom, set_zoom) = signal(100.0f64);
    let (pan_x, set_pan_x) = signal(0.0f64);
    let (pan_y, set_pan_y) = signal(0.0f64);
    let (is_dragging, set_is_dragging) = signal(false);
    let (drag_start_x, set_drag_start_x) = signal(0.0f64);
    let (drag_start_y, set_drag_start_y) = signal(0.0f64);
    let (pan_start_x, set_pan_start_x) = signal(0.0f64);
    let (pan_start_y, set_pan_start_y) = signal(0.0f64);
    let (last_pinch_dist, set_last_pinch_dist) = signal(0.0f64);

    // Scroll sync: true = follow cursor, false = user took manual control
    let (follow_cursor, set_follow_cursor) = signal(true);

    let zoom_in = move |_| { set_follow_cursor.set(false); set_zoom.update(|z| *z = (*z + 10.0).min(500.0)); };
    let zoom_out = move |_| { set_follow_cursor.set(false); set_zoom.update(|z| *z = (*z - 10.0).max(25.0)); };
    let zoom_reset = move |_| { set_zoom.set(100.0); set_pan_x.set(0.0); set_pan_y.set(0.0); set_follow_cursor.set(true); };

    // Sync SVG preview scroll position to cursor ratio
    if let Some(cursor_ratio) = cursor_ratio {
        Effect::new(move |_| {
            let ratio = cursor_ratio.get();
            if !follow_cursor.get() { return; }
            if mode.get_untracked() != PreviewMode::Svg { return; }

            // Scroll the preview container proportionally
            if let Some(window) = web_sys::window() {
                if let Some(document) = window.document() {
                    if let Some(el) = document.query_selector("#preview-area").ok().flatten() {
                        let scroll_height = el.scroll_height() as f64;
                        let client_height = el.client_height() as f64;
                        let max_scroll = (scroll_height - client_height).max(0.0);
                        el.set_scroll_top(ratio * max_scroll);
                    }
                }
            }
        });
    }

    // Attach Ctrl+Wheel zoom
    spawn_local(async move {
        gloo_timers::future::sleep(std::time::Duration::from_millis(100)).await;
        let Some(window) = web_sys::window() else { return };
        let Some(document) = window.document() else { return };
        let Some(el) = document.query_selector("#preview-area").ok().flatten() else { return };
        let el: web_sys::HtmlElement = el.dyn_into().unwrap();

        let wheel_cb = Closure::wrap(Box::new(move |ev: web_sys::WheelEvent| {
            if ev.ctrl_key() || ev.meta_key() {
                ev.prevent_default();
                set_follow_cursor.set(false);
                let step = if ev.delta_y() > 0.0 { -5.0 } else { 5.0 };
                set_zoom.update(|z| *z = (*z + step).clamp(25.0, 500.0));
            }
        }) as Box<dyn FnMut(_)>);

        let opts = web_sys::AddEventListenerOptions::new();
        opts.set_passive(false);
        let _ = el.add_event_listener_with_callback_and_add_event_listener_options(
            "wheel", wheel_cb.as_ref().unchecked_ref(), &opts
        );
        wheel_cb.forget();

        // Touch: pinch zoom + pan
        let touchstart_cb = Closure::wrap(Box::new(move |ev: web_sys::TouchEvent| {
            let touches = ev.touches();
            set_follow_cursor.set(false);
            if touches.length() == 2 {
                ev.prevent_default();
                let t0 = touches.get(0).unwrap();
                let t1 = touches.get(1).unwrap();
                let dx = (t0.client_x() - t1.client_x()) as f64;
                let dy = (t0.client_y() - t1.client_y()) as f64;
                set_last_pinch_dist.set((dx * dx + dy * dy).sqrt());
            } else if touches.length() == 1 {
                let t = touches.get(0).unwrap();
                set_is_dragging.set(true);
                set_drag_start_x.set(t.client_x() as f64);
                set_drag_start_y.set(t.client_y() as f64);
                set_pan_start_x.set(pan_x.get_untracked());
                set_pan_start_y.set(pan_y.get_untracked());
            }
        }) as Box<dyn FnMut(_)>);

        let touchmove_cb = Closure::wrap(Box::new(move |ev: web_sys::TouchEvent| {
            let touches = ev.touches();
            if touches.length() == 2 {
                ev.prevent_default();
                let t0 = touches.get(0).unwrap();
                let t1 = touches.get(1).unwrap();
                let dx = (t0.client_x() - t1.client_x()) as f64;
                let dy = (t0.client_y() - t1.client_y()) as f64;
                let dist = (dx * dx + dy * dy).sqrt();
                let prev = last_pinch_dist.get_untracked();
                if prev > 0.0 {
                    set_zoom.update(|z| *z = (*z * dist / prev).clamp(25.0, 500.0));
                }
                set_last_pinch_dist.set(dist);
            } else if touches.length() == 1 && is_dragging.get_untracked() {
                ev.prevent_default();
                let t = touches.get(0).unwrap();
                set_pan_x.set(pan_start_x.get_untracked() + t.client_x() as f64 - drag_start_x.get_untracked());
                set_pan_y.set(pan_start_y.get_untracked() + t.client_y() as f64 - drag_start_y.get_untracked());
            }
        }) as Box<dyn FnMut(_)>);

        let touchend_cb = Closure::wrap(Box::new(move |_: web_sys::TouchEvent| {
            set_is_dragging.set(false);
            set_last_pinch_dist.set(0.0);
        }) as Box<dyn FnMut(_)>);

        let opts2 = web_sys::AddEventListenerOptions::new();
        opts2.set_passive(false);
        let _ = el.add_event_listener_with_callback_and_add_event_listener_options(
            "touchstart", touchstart_cb.as_ref().unchecked_ref(), &opts2);
        let opts3 = web_sys::AddEventListenerOptions::new();
        opts3.set_passive(false);
        let _ = el.add_event_listener_with_callback_and_add_event_listener_options(
            "touchmove", touchmove_cb.as_ref().unchecked_ref(), &opts3);
        let _ = el.add_event_listener_with_callback("touchend", touchend_cb.as_ref().unchecked_ref());
        touchstart_cb.forget();
        touchmove_cb.forget();
        touchend_cb.forget();
    });

    let has_pdf = move || pdf_blob_url.map(|s| s.get().is_some()).unwrap_or(false);

    view! {
        <div class="flex flex-col h-full">
            // Toolbar
            <div class="flex items-center gap-1 px-2 py-1 bg-base-200 border-b border-base-300 flex-shrink-0">
                <span class="icon-[lucide--eye] text-sm text-accent"></span>
                <span class="text-xs font-semibold uppercase tracking-wide text-base-content/50">"Preview"</span>

                // Mode toggle (SVG / PDF)
                <Show when=has_pdf>
                    <div class="join join-horizontal ml-2">
                        <button
                            class=move || format!("btn btn-xs join-item {}",
                                if mode.get() == PreviewMode::Svg { "btn-primary" } else { "btn-ghost" })
                            on:click=move |_| set_mode.set(PreviewMode::Svg)
                        >"SVG"</button>
                        <button
                            class=move || format!("btn btn-xs join-item {}",
                                if mode.get() == PreviewMode::Pdf { "btn-primary" } else { "btn-ghost" })
                            on:click=move |_| set_mode.set(PreviewMode::Pdf)
                        >"PDF"</button>
                    </div>
                </Show>

                <span class="flex-1"></span>

                // Scroll sync indicator (SVG mode only)
                <Show when=move || mode.get() == PreviewMode::Svg>
                    <button
                        class=move || format!("btn btn-ghost btn-xs {}",
                            if follow_cursor.get() { "text-primary" } else { "text-base-content/30" })
                        title=move || if follow_cursor.get() { "Scroll sync ON (click to disable)" } else { "Scroll sync OFF (click to enable)" }
                        on:click=move |_| set_follow_cursor.update(|v| *v = !*v)
                    >
                        <span class="icon-[lucide--link] text-xs"></span>
                    </button>

                    <button class="btn btn-ghost btn-xs" title="Zoom out" on:click=zoom_out>
                        <span class="icon-[lucide--minus] text-xs"></span>
                    </button>
                    <button class="btn btn-ghost btn-xs font-mono min-w-12 text-xs"
                        title="Reset" on:click=zoom_reset>
                        {move || format!("{}%", zoom.get() as u32)}
                    </button>
                    <button class="btn btn-ghost btn-xs" title="Zoom in" on:click=zoom_in>
                        <span class="icon-[lucide--plus] text-xs"></span>
                    </button>
                    <button class="btn btn-ghost btn-xs" title="Reset view" on:click=zoom_reset>
                        <span class="icon-[lucide--maximize-2] text-xs"></span>
                    </button>
                </Show>
            </div>

            // Content area
            {move || {
                if let Some(err) = error.get() {
                    // Parse out missing packages marker if present
                    let marker = "\n\n[MISSING_PACKAGES:";
                    let (display_err, missing_pkgs) = if let Some(idx) = err.find(marker) {
                        let start = idx + marker.len();
                        let end = err.len() - 1; // strip trailing ]
                        let pkg_str = &err[start..end];
                        let pkgs: Vec<String> = pkg_str.split(',').map(|s| s.to_string()).collect();
                        (err[..idx].to_string(), pkgs)
                    } else {
                        (err.clone(), Vec::new())
                    };

                    let has_missing = !missing_pkgs.is_empty();
                    let pkgs_for_btn = missing_pkgs.clone();

                    view! {
                        <div class="flex-1 overflow-auto bg-base-100 p-4" style="cursor: default;">
                            <div class="space-y-3">
                                <div class="flex items-center gap-2 text-error font-semibold text-sm">
                                    <span class="icon-[lucide--alert-circle] text-lg"></span>
                                    "Compilation Error"
                                </div>
                                <pre class="text-xs font-mono bg-base-200 rounded-lg p-3 overflow-x-auto whitespace-pre-wrap text-error/90">{display_err}</pre>

                                {has_missing.then(move || {
                                    view! { <PackageInstaller packages=pkgs_for_btn.clone() /> }
                                })}
                            </div>
                        </div>
                    }.into_any()
                } else if mode.get() == PreviewMode::Pdf {
                    // PDF viewer via iframe
                    let url = pdf_blob_url
                        .and_then(|s| s.get())
                        .unwrap_or_default();
                    view! {
                        <iframe
                            class="flex-1 w-full border-0"
                            src=url
                        ></iframe>
                    }.into_any()
                } else {
                    // SVG preview with pan/zoom
                    let scale = zoom.get() / 100.0;
                    let tx = pan_x.get();
                    let ty = pan_y.get();
                    view! {
                        <div
                            id="preview-area"
                            class="preview-scroll-area bg-base-100"
                            style:cursor=move || if is_dragging.get() { "grabbing" } else { "default" }

                            on:mousedown=move |ev| {
                                if ev.button() == 0 {
                                    set_follow_cursor.set(false);
                                    set_is_dragging.set(true);
                                    set_drag_start_x.set(ev.client_x() as f64);
                                    set_drag_start_y.set(ev.client_y() as f64);
                                    set_pan_start_x.set(pan_x.get_untracked());
                                    set_pan_start_y.set(pan_y.get_untracked());
                                }
                            }
                            on:mousemove=move |ev| {
                                if is_dragging.get() {
                                    set_pan_x.set(pan_start_x.get_untracked() + ev.client_x() as f64 - drag_start_x.get_untracked());
                                    set_pan_y.set(pan_start_y.get_untracked() + ev.client_y() as f64 - drag_start_y.get_untracked());
                                }
                            }
                            on:mouseup=move |_| set_is_dragging.set(false)
                            on:mouseleave=move |_| set_is_dragging.set(false)
                            on:dblclick=move |_| {
                                set_zoom.set(100.0);
                                set_pan_x.set(0.0);
                                set_pan_y.set(0.0);
                                set_follow_cursor.set(true);
                            }
                        >
                            <div
                                class="preview-content"
                                style:transform=format!("translate({}px, {}px) scale({})", tx, ty, scale)
                                style:transform-origin="top center"
                                inner_html=move || output.get()
                            ></div>
                        </div>
                    }.into_any()
                }
            }}
        </div>
    }
}

/// Inline package installer with progress bar and expandable log
#[component]
fn PackageInstaller(packages: Vec<String>) -> impl IntoView {
    let (is_installing, set_is_installing) = signal(false);
    let (is_done, set_is_done) = signal(false);
    let (progress, set_progress) = signal(0u32); // 0-100
    let (log_lines, set_log_lines) = signal(Vec::<String>::new());
    let (latest_line, set_latest_line) = signal(String::new());
    let (log_expanded, set_log_expanded) = signal(true); // expanded by default

    let total = packages.len();
    let pkgs = packages.clone();
    let pkgs_display = packages.clone();

    let pkgs_signal = RwSignal::new(pkgs);
    let install_cache = RwSignal::new(use_context::<AppState>().expect("AppState").package_cache.clone());
    let start_install = move |_: leptos::ev::MouseEvent| {
        let pkgs = pkgs_signal.get_untracked();
        let cache = install_cache.get_untracked();
        set_is_installing.set(true);
        set_is_done.set(false);
        set_log_lines.set(Vec::new());
        set_progress.set(0);

        spawn_local(async move {
            let total = pkgs.len();
            let mut installed = 0;

            add_log(&set_log_lines, &set_latest_line,
                format!("Starting installation of {} package{}...", total, if total != 1 { "s" } else { "" }));

            if total == 0 {
                add_log(&set_log_lines, &set_latest_line, "No packages to install".to_string());
                set_is_done.set(true);
                set_is_installing.set(false);
                return;
            }

            for (i, pkg_str) in pkgs.iter().enumerate() {
                let Some(spec) = crate::packages::registry::PkgSpec::parse(pkg_str) else {
                    add_log(&set_log_lines, &set_latest_line, format!("Skipping invalid spec: {}", pkg_str));
                    set_progress.set(((i + 1) * 100 / total) as u32);
                    continue;
                };

                if cache.has_package(&spec) {
                    add_log(&set_log_lines, &set_latest_line, format!("[{}/{}] {} already installed", i+1, total, spec.to_string()));
                    installed += 1;
                    set_progress.set(((i + 1) * 100 / total) as u32);
                    continue;
                }

                add_log(&set_log_lines, &set_latest_line,
                    format!("[{}/{}] Downloading {}...", i+1, total, spec.to_string()));

                let url = spec.tar_url();
                add_log(&set_log_lines, &set_latest_line, format!("  URL: {}", url));

                match crate::packages::download_package(&spec).await {
                    Ok(files) => {
                        let count = files.len();
                        add_log(&set_log_lines, &set_latest_line, format!("  Extracted {} files", count));

                        // Show some file names
                        let sample: Vec<&String> = files.keys().take(5).collect();
                        for f in &sample {
                            add_log(&set_log_lines, &set_latest_line, format!("    {}", f));
                        }
                        if files.len() > 5 {
                            add_log(&set_log_lines, &set_latest_line, format!("    ... and {} more", files.len() - 5));
                        }

                        match cache.store_package(&spec, files).await {
                            Ok(()) => {
                                add_log(&set_log_lines, &set_latest_line,
                                    format!("  Cached {} successfully", spec.to_string()));
                                installed += 1;
                            }
                            Err(e) => add_log(&set_log_lines, &set_latest_line, format!("  Cache error: {}", e)),
                        }
                    }
                    Err(e) => {
                        add_log(&set_log_lines, &set_latest_line,
                            format!("  FAILED: {}", e));
                        if e.contains("Fetch error") || e.contains("TypeError") {
                            add_log(&set_log_lines, &set_latest_line,
                                "  This is likely a CORS error. The Typst package registry may not allow browser downloads.".to_string());
                            add_log(&set_log_lines, &set_latest_line,
                                "  Workaround: download packages manually and add them via the Package Manager.".to_string());
                        }
                    }
                }
                set_progress.set(((i + 1) * 100 / total) as u32);
            }

            if installed == total {
                add_log(&set_log_lines, &set_latest_line,
                    format!("All {} packages installed successfully!", total));
            } else {
                add_log(&set_log_lines, &set_latest_line,
                    format!("Done. {}/{} packages installed.", installed, total));
            }
            set_is_done.set(true);
            set_is_installing.set(false);
        });
    };

    view! {
        <div class="bg-info/10 border border-info/30 rounded-lg p-3">
            <div class="flex items-center gap-2 text-info text-sm font-medium mb-2">
                <span class="icon-[lucide--package] text-lg"></span>
                {format!("{} missing package{}", total, if total != 1 { "s" } else { "" })}
            </div>

            // Package list
            <div class="text-xs text-base-content/70 mb-3 font-mono">
                {pkgs_display.iter().map(|p| format!("  {}", p)).collect::<Vec<_>>().join("\n")}
            </div>

            // Install button (hidden when done)
            <Show when=move || !is_installing.get() && !is_done.get()>
                <button class="btn btn-info btn-sm gap-2" on:click=start_install>
                    <span class="icon-[lucide--download] text-sm"></span>
                    "Install Packages"
                </button>
            </Show>

            // Progress bar + latest log line
            <Show when=move || is_installing.get() || is_done.get()>
                <div class="space-y-2">
                    // Progress bar
                    <div class="flex items-center gap-2">
                        <progress class="progress progress-info flex-1 h-2" value=move || progress.get() max="100"></progress>
                        <span class="text-xs font-mono text-base-content/50">{move || format!("{}%", progress.get())}</span>
                    </div>

                    // Latest log line
                    <div class="flex items-center gap-2">
                        <Show when=move || is_installing.get()>
                            <span class="loading loading-spinner loading-xs text-info"></span>
                        </Show>
                        <Show when=move || is_done.get()>
                            <span class="icon-[lucide--check-circle] text-sm text-success"></span>
                        </Show>
                        <span class="text-xs text-base-content/70 truncate flex-1">{move || latest_line.get()}</span>
                        <button class="btn btn-ghost btn-xs" title="Toggle log"
                            on:click=move |_| set_log_expanded.update(|v| *v = !*v)>
                            <span class=move || if log_expanded.get() {
                                "icon-[lucide--chevron-up] text-xs"
                            } else {
                                "icon-[lucide--chevron-down] text-xs"
                            }></span>
                        </button>
                    </div>

                    // Expandable full log
                    <Show when=move || log_expanded.get()>
                        <pre class="text-[10px] font-mono bg-base-300/30 rounded p-2 max-h-40 overflow-y-auto text-base-content/60">
                            {move || log_lines.get().join("\n")}
                        </pre>
                    </Show>
                </div>
            </Show>
        </div>
    }
}

fn add_log(set_lines: &WriteSignal<Vec<String>>, set_latest: &WriteSignal<String>, msg: String) {
    log::info!("[pkg] {}", msg);
    set_latest.set(msg.clone());
    set_lines.update(|lines| lines.push(msg));
}
