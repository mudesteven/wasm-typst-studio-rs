use leptos::prelude::*;
use wasm_typst_studio_rs::App;

fn main() {
    // set up logging
    _ = console_log::init_with_level(log::Level::Debug);
    console_error_panic_hook::set_once();

    // Check if we're in a worker context (workers don't have a document)
    // If in worker, don't mount the app
    if web_sys::window().and_then(|w| w.document()).is_some() {
        mount_to_body(|| {
            view! {
                <App />
            }
        })
    } else {
        // We're in a worker context - skip mounting
        log::info!("Worker context detected - skipping DOM mount");
    }
}
