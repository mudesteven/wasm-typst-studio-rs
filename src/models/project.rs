use serde::{Deserialize, Serialize};

/// A project containing multiple files
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Project {
    pub id: String,
    pub name: String,
    pub main_file: String,
    pub created_at: f64,
    pub updated_at: f64,
}

impl Project {
    pub fn new(name: String) -> Self {
        let now = js_sys::Date::now();
        let id = generate_id();
        Self {
            id,
            name,
            main_file: "main.typ".to_string(),
            created_at: now,
            updated_at: now,
        }
    }
}

/// Summary info for listing projects
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProjectMetadata {
    pub id: String,
    pub name: String,
    pub main_file: String,
    pub created_at: f64,
    pub updated_at: f64,
    pub file_count: usize,
}

impl From<&Project> for ProjectMetadata {
    fn from(p: &Project) -> Self {
        Self {
            id: p.id.clone(),
            name: p.name.clone(),
            main_file: p.main_file.clone(),
            created_at: p.created_at,
            updated_at: p.updated_at,
            file_count: 0,
        }
    }
}

/// A file within a project
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProjectFile {
    pub path: String,
    pub content: FileContent,
}

/// File content - text or binary
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum FileContent {
    Text(String),
    Binary(Vec<u8>),
}

impl FileContent {
    pub fn as_text(&self) -> Option<&str> {
        match self {
            FileContent::Text(s) => Some(s),
            FileContent::Binary(_) => None,
        }
    }

    pub fn as_bytes(&self) -> Vec<u8> {
        match self {
            FileContent::Text(s) => s.as_bytes().to_vec(),
            FileContent::Binary(b) => b.clone(),
        }
    }

    pub fn is_text(&self) -> bool {
        matches!(self, FileContent::Text(_))
    }
}

/// Generate a simple unique ID using timestamp + random
fn generate_id() -> String {
    let timestamp = js_sys::Date::now() as u64;
    let random = (js_sys::Math::random() * 1_000_000.0) as u32;
    format!("{:x}_{:06x}", timestamp, random)
}

/// Determine if a file path is a text file based on extension
pub fn is_text_file(path: &str) -> bool {
    let lower = path.to_lowercase();
    lower.ends_with(".typ")
        || lower.ends_with(".yml")
        || lower.ends_with(".yaml")
        || lower.ends_with(".bib")
        || lower.ends_with(".txt")
        || lower.ends_with(".csv")
        || lower.ends_with(".json")
        || lower.ends_with(".toml")
}

/// Determine if a file path is an image based on extension
pub fn is_image_file(path: &str) -> bool {
    let lower = path.to_lowercase();
    lower.ends_with(".png")
        || lower.ends_with(".jpg")
        || lower.ends_with(".jpeg")
        || lower.ends_with(".gif")
        || lower.ends_with(".svg")
        || lower.ends_with(".webp")
}
