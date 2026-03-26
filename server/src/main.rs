use axum::{
    extract::Path,
    http::StatusCode,
    response::Json,
    routing::{delete, get, patch, post, put},
    Router,
};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use tower_http::cors::CorsLayer;

const PROJECTS_DIR: &str = "data/projects";

#[derive(Serialize, Deserialize)]
struct ProjectMeta {
    id: String,
    name: String,
    main_file: String,
    created_at: f64,
    updated_at: f64,
    file_count: usize,
}

#[derive(Deserialize)]
struct CreateProject {
    name: String,
}

#[derive(Deserialize)]
struct RenameFile {
    new_path: String,
}

fn projects_root() -> PathBuf {
    PathBuf::from(PROJECTS_DIR)
}

fn project_dir(id: &str) -> PathBuf {
    projects_root().join(id)
}

/// List all projects
async fn list_projects() -> Result<Json<Vec<ProjectMeta>>, StatusCode> {
    let root = projects_root();
    if !root.exists() {
        return Ok(Json(Vec::new()));
    }

    let mut projects = Vec::new();
    for entry in fs::read_dir(&root).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)? {
        let entry = entry.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        if entry.path().is_dir() {
            let meta_path = entry.path().join("project.json");
            if meta_path.exists() {
                let data = fs::read_to_string(&meta_path).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
                if let Ok(meta) = serde_json::from_str::<ProjectMeta>(&data) {
                    projects.push(meta);
                }
            }
        }
    }

    projects.sort_by(|a, b| b.updated_at.partial_cmp(&a.updated_at).unwrap_or(std::cmp::Ordering::Equal));
    Ok(Json(projects))
}

/// Get a single project
async fn get_project(Path(id): Path<String>) -> Result<Json<ProjectMeta>, StatusCode> {
    let meta_path = project_dir(&id).join("project.json");
    let data = fs::read_to_string(&meta_path).map_err(|_| StatusCode::NOT_FOUND)?;
    let meta: ProjectMeta = serde_json::from_str(&data).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(meta))
}

/// Create a new project
async fn create_project(Json(body): Json<CreateProject>) -> Result<(StatusCode, Json<ProjectMeta>), StatusCode> {
    let id = format!("{:x}_{:06x}",
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis(),
        rand_u32() % 0xFFFFFF
    );

    let dir = project_dir(&id);
    fs::create_dir_all(&dir).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as f64;

    let meta = ProjectMeta {
        id: id.clone(),
        name: body.name,
        main_file: "main.typ".to_string(),
        created_at: now,
        updated_at: now,
        file_count: 1,
    };

    let meta_json = serde_json::to_string_pretty(&meta).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    fs::write(dir.join("project.json"), &meta_json).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Create default main.typ
    fs::write(dir.join("main.typ"), "= Hello World\n\nStart writing here.\n")
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok((StatusCode::CREATED, Json(meta)))
}

/// Delete a project
async fn delete_project(Path(id): Path<String>) -> Result<StatusCode, StatusCode> {
    let dir = project_dir(&id);
    if dir.exists() {
        fs::remove_dir_all(&dir).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    }
    Ok(StatusCode::NO_CONTENT)
}

/// Update project metadata
async fn update_project(Path(id): Path<String>, Json(meta): Json<ProjectMeta>) -> Result<StatusCode, StatusCode> {
    let dir = project_dir(&id);
    if !dir.exists() {
        return Err(StatusCode::NOT_FOUND);
    }
    let meta_json = serde_json::to_string_pretty(&meta).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    fs::write(dir.join("project.json"), &meta_json).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(StatusCode::OK)
}

/// List files in a project (recursive)
async fn list_files(Path(id): Path<String>) -> Result<Json<Vec<String>>, StatusCode> {
    let dir = project_dir(&id);
    if !dir.exists() {
        return Err(StatusCode::NOT_FOUND);
    }

    let mut files = Vec::new();
    collect_files(&dir, &dir, &mut files);
    files.sort();
    Ok(Json(files))
}

fn collect_files(base: &PathBuf, current: &PathBuf, files: &mut Vec<String>) {
    if let Ok(entries) = fs::read_dir(current) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                collect_files(base, &path, files);
            } else {
                let name = path.file_name().unwrap_or_default().to_string_lossy().to_string();
                if name == "project.json" {
                    continue; // Skip metadata file
                }
                if let Ok(relative) = path.strip_prefix(base) {
                    files.push(relative.to_string_lossy().to_string());
                }
            }
        }
    }
}

/// Read a file
async fn read_file(Path((id, file_path)): Path<(String, String)>) -> Result<Vec<u8>, StatusCode> {
    let path = project_dir(&id).join(&file_path);
    fs::read(&path).map_err(|_| StatusCode::NOT_FOUND)
}

/// Write a file
async fn write_file(
    Path((id, file_path)): Path<(String, String)>,
    body: axum::body::Bytes,
) -> Result<StatusCode, StatusCode> {
    let path = project_dir(&id).join(&file_path);

    // Create parent directories if needed
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    }

    fs::write(&path, &body).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Update file count in project.json
    update_file_count(&id);

    Ok(StatusCode::OK)
}

/// Delete a file
async fn delete_file_handler(Path((id, file_path)): Path<(String, String)>) -> Result<StatusCode, StatusCode> {
    let path = project_dir(&id).join(&file_path);
    if path.exists() {
        fs::remove_file(&path).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    }
    update_file_count(&id);
    Ok(StatusCode::NO_CONTENT)
}

/// Rename a file
async fn rename_file(
    Path((id, file_path)): Path<(String, String)>,
    Json(body): Json<RenameFile>,
) -> Result<StatusCode, StatusCode> {
    let old_path = project_dir(&id).join(&file_path);
    let new_path = project_dir(&id).join(&body.new_path);

    if !old_path.exists() {
        return Err(StatusCode::NOT_FOUND);
    }

    if let Some(parent) = new_path.parent() {
        fs::create_dir_all(parent).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    }

    fs::rename(&old_path, &new_path).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(StatusCode::OK)
}

fn update_file_count(id: &str) {
    let dir = project_dir(id);
    let meta_path = dir.join("project.json");
    if let Ok(data) = fs::read_to_string(&meta_path) {
        if let Ok(mut meta) = serde_json::from_str::<ProjectMeta>(&data) {
            let mut files = Vec::new();
            collect_files(&dir, &dir, &mut files);
            meta.file_count = files.len();
            if let Ok(json) = serde_json::to_string_pretty(&meta) {
                let _ = fs::write(&meta_path, json);
            }
        }
    }
}

fn rand_u32() -> u32 {
    // Simple pseudo-random using system time nanoseconds
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .subsec_nanos()
}

#[tokio::main]
async fn main() {
    // Ensure projects directory exists
    fs::create_dir_all(PROJECTS_DIR).expect("Failed to create projects directory");

    let app = Router::new()
        .route("/api/projects", get(list_projects).post(create_project))
        .route("/api/projects/{id}", get(get_project).put(update_project).delete(delete_project))
        .route("/api/projects/{id}/files", get(list_files))
        .route("/api/projects/{id}/files/{*file_path}", get(read_file).put(write_file).delete(delete_file_handler).patch(rename_file))
        .layer(CorsLayer::permissive());

    let addr = "0.0.0.0:3001";
    println!("Typst Studio file server running on http://{}", addr);
    println!("Projects stored in: {}/", PROJECTS_DIR);

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
