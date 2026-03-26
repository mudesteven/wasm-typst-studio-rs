use leptos::prelude::*;

/// Google Drive OAuth2 authentication
#[derive(Clone, Debug)]
pub struct GDriveAuth {
    pub access_token: RwSignal<Option<String>>,
    pub is_authenticated: RwSignal<bool>,
    pub user_email: RwSignal<Option<String>>,
}

impl GDriveAuth {
    pub fn new() -> Self {
        let auth = Self {
            access_token: RwSignal::new(None),
            is_authenticated: RwSignal::new(false),
            user_email: RwSignal::new(None),
        };
        auth.restore_session();
        auth.check_redirect();
        auth
    }

    /// Start OAuth2 implicit grant flow
    /// Redirects the user to Google's OAuth consent screen
    pub fn start_auth(&self, client_id: &str) {
        let redirect_uri = current_origin();
        let scope = "https://www.googleapis.com/auth/drive.file";

        let auth_url = format!(
            "https://accounts.google.com/o/oauth2/v2/auth?\
             client_id={}&\
             redirect_uri={}&\
             response_type=token&\
             scope={}&\
             prompt=consent",
            client_id,
            js_sys::encode_uri_component(&redirect_uri),
            js_sys::encode_uri_component(scope),
        );

        if let Some(window) = web_sys::window() {
            let _ = window.location().set_href(&auth_url);
        }
    }

    /// Check URL fragment for OAuth token after redirect
    fn check_redirect(&self) {
        if let Some(window) = web_sys::window() {
            if let Ok(hash) = window.location().hash() {
                if hash.contains("access_token=") {
                    // Parse token from fragment: #access_token=...&token_type=...&expires_in=...
                    let params: std::collections::HashMap<String, String> = hash
                        .trim_start_matches('#')
                        .split('&')
                        .filter_map(|pair| {
                            let mut parts = pair.splitn(2, '=');
                            Some((parts.next()?.to_string(), parts.next()?.to_string()))
                        })
                        .collect();

                    if let Some(token) = params.get("access_token") {
                        self.access_token.set(Some(token.clone()));
                        self.is_authenticated.set(true);
                        self.save_session(token);

                        // Clear the hash to avoid re-processing
                        let _ = window.location().set_hash("");
                        log::info!("Google Drive authenticated via redirect");
                    }
                }
            }
        }
    }

    /// Save token to sessionStorage
    fn save_session(&self, token: &str) {
        if let Some(window) = web_sys::window() {
            if let Ok(Some(storage)) = window.session_storage() {
                let _ = storage.set_item("gdrive_token", token);
            }
        }
    }

    /// Restore token from sessionStorage
    pub fn restore_session(&self) {
        if let Some(window) = web_sys::window() {
            if let Ok(Some(storage)) = window.session_storage() {
                if let Ok(Some(token)) = storage.get_item("gdrive_token") {
                    self.access_token.set(Some(token));
                    self.is_authenticated.set(true);
                }
            }
        }
    }

    /// Sign out and clear session
    pub fn sign_out(&self) {
        self.access_token.set(None);
        self.is_authenticated.set(false);
        self.user_email.set(None);
        if let Some(window) = web_sys::window() {
            if let Ok(Some(storage)) = window.session_storage() {
                let _ = storage.remove_item("gdrive_token");
            }
        }
    }
}

fn current_origin() -> String {
    web_sys::window()
        .and_then(|w| w.location().origin().ok())
        .unwrap_or_else(|| "http://localhost:1420".to_string())
}
