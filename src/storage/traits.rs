use crate::models::{Project, ProjectMetadata, FileContent};

/// Async storage trait for project data.
/// All backends (IndexedDB, server API, Tauri FS) implement this trait.
///
/// Note: We use Pin<Box<dyn Future>> instead of async-trait to avoid
/// the extra dependency, since WASM is single-threaded anyway.
pub trait ProjectStorage {
    fn list_projects(&self) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Vec<ProjectMetadata>, String>>>>;
    fn get_project(&self, id: &str) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Project, String>>>>;
    fn create_project(&self, name: &str) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Project, String>>>>;
    fn delete_project(&self, id: &str) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), String>>>>;
    fn update_project(&self, project: &Project) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), String>>>>;

    fn list_files(&self, project_id: &str) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Vec<String>, String>>>>;
    fn read_file(&self, project_id: &str, path: &str) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<FileContent, String>>>>;
    fn write_file(&self, project_id: &str, path: &str, content: &FileContent) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), String>>>>;
    fn delete_file(&self, project_id: &str, path: &str) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), String>>>>;
    fn rename_file(&self, project_id: &str, old_path: &str, new_path: &str) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), String>>>>;
}
