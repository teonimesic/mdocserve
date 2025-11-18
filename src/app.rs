use anyhow::Result;
use axum::{
    extract::{
        ws::{Message, WebSocket},
        Path as AxumPath, State, WebSocketUpgrade,
    },
    http::{header, StatusCode, Uri},
    response::{IntoResponse, Json, Response},
    routing::{get, put},
    Router,
};
use futures_util::{SinkExt, StreamExt};
use notify::{Config, Event, RecommendedWatcher, RecursiveMode, Watcher};
use rust_embed::RustEmbed;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    net::Ipv6Addr,
    path::{Path, PathBuf},
    sync::Arc,
    time::SystemTime,
};
use tokio::{
    net::TcpListener,
    sync::{broadcast, mpsc, Mutex},
};
use tower_http::cors::CorsLayer;

#[derive(RustEmbed)]
#[folder = "frontend/dist"]
struct FrontendAssets;

const RESCAN_DELAY_MS: u64 = 200;

type SharedMarkdownState = Arc<Mutex<MarkdownState>>;

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "type")]
pub enum ClientMessage {
    Ping,
    RequestRefresh,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(tag = "type")]
pub enum ServerMessage {
    Reload,
    Pong,
    FileAdded { name: String },
    FileModified { name: String },
    FileRenamed { old_name: String, new_name: String },
    FileRemoved { name: String },
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ApiFile {
    path: String,
}

#[derive(Serialize, Debug)]
struct FilesResponse {
    files: Vec<ApiFile>,
}

#[derive(Serialize, Debug)]
struct FileContentResponse {
    markdown: String,
}

#[derive(Deserialize, Debug)]
struct FileUpdateRequest {
    markdown: String,
}

pub fn scan_markdown_files(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut md_files = Vec::new();
    scan_markdown_files_recursive(dir, &mut md_files)?;
    md_files.sort();
    Ok(md_files)
}

fn scan_markdown_files_recursive(dir: &Path, md_files: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();

        if path.is_file() && is_markdown_file(&path) {
            md_files.push(path);
        } else if path.is_dir() {
            scan_markdown_files_recursive(&path, md_files)?;
        }
    }

    Ok(())
}

fn is_markdown_file(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.eq_ignore_ascii_case("md") || ext.eq_ignore_ascii_case("markdown"))
        .unwrap_or(false)
}

fn calculate_relative_path(file_path: &Path, base_dir: &Path) -> Result<String> {
    let canonical_path = file_path.canonicalize()?;
    let relative_path = canonical_path
        .strip_prefix(base_dir)
        .map_err(|_| anyhow::anyhow!("File path is not within base directory"))?
        .to_string_lossy()
        .to_string();
    Ok(relative_path)
}

struct TrackedFile {
    path: PathBuf,
    last_modified: SystemTime,
    markdown: String,
    content_hash: md5::Digest,
}

struct MarkdownState {
    base_dir: PathBuf,
    tracked_files: std::collections::HashMap<String, TrackedFile>,
    is_directory_mode: bool,
    change_tx: broadcast::Sender<ServerMessage>,
}

impl MarkdownState {
    fn new(base_dir: PathBuf, file_paths: Vec<PathBuf>, is_directory_mode: bool) -> Result<Self> {
        let (change_tx, _) = broadcast::channel::<ServerMessage>(16);

        let mut tracked_files = std::collections::HashMap::new();
        for file_path in file_paths {
            let metadata = fs::metadata(&file_path)?;
            let last_modified = metadata.modified()?;
            let content = fs::read_to_string(&file_path)?;
            let content_hash = md5::compute(&content);
            let relative_path = calculate_relative_path(&file_path, &base_dir)?;

            tracked_files.insert(
                relative_path.clone(),
                TrackedFile {
                    path: file_path,
                    last_modified,
                    markdown: content,
                    content_hash,
                },
            );
        }

        Ok(MarkdownState {
            base_dir,
            tracked_files,
            is_directory_mode,
            change_tx,
        })
    }

    fn get_sorted_filenames(&self) -> Vec<String> {
        let mut filenames: Vec<_> = self.tracked_files.keys().cloned().collect();
        filenames.sort();
        filenames
    }

    fn refresh_file(&mut self, relative_path: &str) -> Result<bool> {
        if let Some(tracked) = self.tracked_files.get_mut(relative_path) {
            let metadata = fs::metadata(&tracked.path)?;
            let current_modified = metadata.modified()?;

            if current_modified > tracked.last_modified {
                let content = fs::read_to_string(&tracked.path)?;
                tracked.markdown = content;
                tracked.last_modified = current_modified;
                return Ok(true); // File was refreshed
            }
        }

        Ok(false) // File was not refreshed
    }

    fn update_file(&mut self, relative_path: &str, new_content: &str) -> Result<()> {
        if let Some(tracked) = self.tracked_files.get_mut(relative_path) {
            fs::write(&tracked.path, new_content)?;
            tracked.markdown = new_content.to_string();
            tracked.last_modified = SystemTime::now();
            tracked.content_hash = md5::compute(new_content.as_bytes());
            let _ = self.change_tx.send(ServerMessage::Reload);
        } else {
            return Err(anyhow::anyhow!("File not found: {}", relative_path));
        }

        Ok(())
    }

    fn add_tracked_file(&mut self, file_path: PathBuf) -> Result<()> {
        let relative_path = calculate_relative_path(&file_path, &self.base_dir)?;

        if self.tracked_files.contains_key(&relative_path) {
            return Ok(());
        }

        let metadata = fs::metadata(&file_path)?;
        let content = fs::read_to_string(&file_path)?;
        let content_hash = md5::compute(&content);

        self.tracked_files.insert(
            relative_path.clone(),
            TrackedFile {
                path: file_path,
                last_modified: metadata.modified()?,
                markdown: content,
                content_hash,
            },
        );

        Ok(())
    }

    fn rescan_directory(&mut self) -> Result<bool> {
        if !self.is_directory_mode {
            return Ok(false);
        }

        let current_files = scan_markdown_files(&self.base_dir)?;
        let current_relative_paths: std::collections::HashSet<String> = current_files
            .iter()
            .filter_map(|p| {
                p.canonicalize().ok().and_then(|canonical| {
                    canonical
                        .strip_prefix(&self.base_dir)
                        .ok()
                        .map(|rel| rel.to_string_lossy().to_string())
                })
            })
            .collect();

        let tracked_relative_paths: std::collections::HashSet<String> =
            self.tracked_files.keys().cloned().collect();

        if current_relative_paths == tracked_relative_paths {
            return Ok(false);
        }

        self.tracked_files
            .retain(|relative_path, _| current_relative_paths.contains(relative_path));

        for file_path in current_files {
            let Ok(canonical_path) = file_path.canonicalize() else {
                continue;
            };
            let Ok(rel_path) = canonical_path.strip_prefix(&self.base_dir) else {
                continue;
            };
            let relative_path = rel_path.to_string_lossy().to_string();

            if self.tracked_files.contains_key(&relative_path) {
                continue;
            }

            let Ok(metadata) = fs::metadata(&file_path) else {
                continue;
            };
            let Ok(content) = fs::read_to_string(&file_path) else {
                continue;
            };
            let Ok(last_modified) = metadata.modified() else {
                continue;
            };
            let content_hash = md5::compute(&content);

            self.tracked_files.insert(
                relative_path.clone(),
                TrackedFile {
                    path: file_path,
                    last_modified,
                    markdown: content,
                    content_hash,
                },
            );
        }

        Ok(true)
    }
}

async fn handle_markdown_file_change(path: &Path, state: &SharedMarkdownState) {
    if !is_markdown_file(path) {
        return;
    }

    let mut state_guard = state.lock().await;

    let Ok(relative_path) = calculate_relative_path(path, &state_guard.base_dir) else {
        return;
    };

    if state_guard.tracked_files.contains_key(&relative_path) {
        if let Ok(was_refreshed) = state_guard.refresh_file(&relative_path) {
            if was_refreshed {
                let _ = state_guard.change_tx.send(ServerMessage::FileModified {
                    name: relative_path,
                });
            }
        }
    } else if state_guard.is_directory_mode
        && state_guard.add_tracked_file(path.to_path_buf()).is_ok()
    {
        let _ = state_guard.change_tx.send(ServerMessage::FileAdded {
            name: relative_path,
        });
    }
}

enum FileChangeType {
    Renamed { old_name: String, new_name: String },
    Removed { name: String },
    Added { name: String },
    Other,
}

fn detect_file_change(
    old_files: &std::collections::HashSet<String>,
    new_files: &std::collections::HashSet<String>,
    _old_tracked_files: &std::collections::HashMap<String, md5::Digest>,
    _new_tracked_files: &std::collections::HashMap<String, TrackedFile>,
) -> FileChangeType {
    let added: Vec<_> = new_files.difference(old_files).collect();
    let removed: Vec<_> = old_files.difference(new_files).collect();

    // If exactly one file was removed and one was added, treat it as a rename
    // This handles both: (1) actual renames with no content change, and
    // (2) files that were edited then renamed (content hash differs)
    if let ([new_name], [old_name]) = (added.as_slice(), removed.as_slice()) {
        return FileChangeType::Renamed {
            old_name: (*old_name).clone(),
            new_name: (*new_name).clone(),
        };
    }

    if let Some(&first_removed) = removed.first() {
        return FileChangeType::Removed {
            name: first_removed.clone(),
        };
    }

    if let Some(&first_added) = added.first() {
        return FileChangeType::Added {
            name: first_added.clone(),
        };
    }

    FileChangeType::Other
}

fn send_change_message(change_type: FileChangeType, tx: &broadcast::Sender<ServerMessage>) {
    let message = match change_type {
        FileChangeType::Renamed { old_name, new_name } => {
            ServerMessage::FileRenamed { old_name, new_name }
        }
        FileChangeType::Removed { name } => ServerMessage::FileRemoved { name },
        FileChangeType::Added { name } => ServerMessage::FileAdded { name },
        FileChangeType::Other => ServerMessage::Reload,
    };

    let _ = tx.send(message);
}

async fn rescan_and_detect_changes(state: &SharedMarkdownState) {
    let (old_files, old_hashes) = {
        let guard = state.lock().await;
        let files = guard.tracked_files.keys().cloned().collect();
        let hashes: std::collections::HashMap<String, md5::Digest> = guard
            .tracked_files
            .iter()
            .map(|(k, v)| (k.clone(), v.content_hash))
            .collect();
        (files, hashes)
    };

    let mut guard = state.lock().await;

    let Ok(changed) = guard.rescan_directory() else {
        return;
    };

    if !changed {
        return;
    }

    let new_files: std::collections::HashSet<String> =
        guard.tracked_files.keys().cloned().collect();

    let change_type = detect_file_change(&old_files, &new_files, &old_hashes, &guard.tracked_files);
    send_change_message(change_type, &guard.change_tx);
}

fn schedule_delayed_rescan(state: &SharedMarkdownState) {
    let state_clone = state.clone();
    tokio::spawn(async move {
        tokio::time::sleep(tokio::time::Duration::from_millis(RESCAN_DELAY_MS)).await;
        rescan_and_detect_changes(&state_clone).await;
    });
}

async fn handle_rename_event(
    mode: notify::event::RenameMode,
    paths: &[PathBuf],
    state: &SharedMarkdownState,
) {
    use notify::event::RenameMode;

    let is_dir_mode = state.lock().await.is_directory_mode;
    if is_dir_mode {
        schedule_delayed_rescan(state);
        return;
    }

    match mode {
        RenameMode::Both => {
            let Some(new_path) = paths.get(1) else { return };
            handle_markdown_file_change(new_path, state).await;
        }
        RenameMode::To => {
            let Some(path) = paths.first() else { return };
            handle_markdown_file_change(path, state).await;
        }
        RenameMode::Any => {
            let Some(path) = paths.first() else { return };
            if !path.exists() {
                return;
            }
            handle_markdown_file_change(path, state).await;
        }
        RenameMode::From | RenameMode::Other => {}
    }
}

async fn handle_md_create_or_modify(path: &Path, state: &SharedMarkdownState) {
    handle_markdown_file_change(path, state).await;
}

async fn handle_md_remove(_path: &Path, state: &SharedMarkdownState) {
    let is_dir_mode = state.lock().await.is_directory_mode;
    if !is_dir_mode {
        return;
    }

    schedule_delayed_rescan(state);
}

async fn handle_image_change(state: &SharedMarkdownState) {
    let guard = state.lock().await;
    let _ = guard.change_tx.send(ServerMessage::Reload);
}

async fn handle_file_event(event: Event, state: &SharedMarkdownState) {
    use notify::event::ModifyKind;
    use notify::EventKind::{Create, Modify, Remove};

    match event.kind {
        Modify(ModifyKind::Name(rename_mode)) => {
            handle_rename_event(rename_mode, &event.paths, state).await;
        }
        _ => {
            for path in &event.paths {
                if is_markdown_file(path) {
                    match event.kind {
                        Create(_) | Modify(ModifyKind::Data(_)) => {
                            handle_md_create_or_modify(path, state).await;
                        }
                        Remove(_) => {
                            handle_md_remove(path, state).await;
                        }
                        _ => {}
                    }
                } else if path.is_file() && is_image_file(path.to_str().unwrap_or("")) {
                    match event.kind {
                        Modify(_) | Create(_) | Remove(_) => {
                            handle_image_change(state).await;
                        }
                        _ => {}
                    }
                }
            }
        }
    }
}

async fn serve_embedded_file(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');

    // If path is empty or is a directory-like path, serve index.html
    let file_path = if path.is_empty() || path.ends_with('/') {
        "index.html"
    } else {
        path
    };

    match FrontendAssets::get(file_path) {
        Some(content) => {
            let mime = mime_guess::from_path(file_path).first_or_octet_stream();
            Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, mime.as_ref())
                .body(content.data.into())
                .unwrap()
        }
        None => {
            // For SPA routing: if the file doesn't exist, serve index.html
            match FrontendAssets::get("index.html") {
                Some(content) => Response::builder()
                    .status(StatusCode::OK)
                    .header(header::CONTENT_TYPE, "text/html")
                    .body(content.data.into())
                    .unwrap(),
                None => Response::builder()
                    .status(StatusCode::NOT_FOUND)
                    .body("404 Not Found".into())
                    .unwrap(),
            }
        }
    }
}

pub fn new_router(
    base_dir: PathBuf,
    tracked_files: Vec<PathBuf>,
    is_directory_mode: bool,
) -> Result<Router> {
    let base_dir = base_dir.canonicalize()?;

    let state = Arc::new(Mutex::new(MarkdownState::new(
        base_dir.clone(),
        tracked_files,
        is_directory_mode,
    )?));

    let watcher_state = state.clone();
    let (tx, mut rx) = mpsc::channel(100);

    let mut watcher = RecommendedWatcher::new(
        move |res: std::result::Result<Event, notify::Error>| {
            if let Ok(event) = res {
                let _ = tx.blocking_send(event);
            }
        },
        Config::default(),
    )?;

    watcher.watch(&base_dir, RecursiveMode::Recursive)?;

    tokio::spawn(async move {
        let _watcher = watcher;
        while let Some(event) = rx.recv().await {
            handle_file_event(event, &watcher_state).await;
        }
    });

    // Create router with embedded frontend assets
    let api_router = Router::new()
        .route("/api/files", get(api_get_files))
        .route("/api/files/*path", get(api_get_file))
        .route("/api/files/*path", put(api_update_file))
        .route("/api/static/*path", get(api_serve_static))
        .route("/ws", get(websocket_handler))
        .route("/__health", get(server_health))
        .with_state(state.clone())
        .fallback(serve_embedded_file);

    Ok(api_router.layer(CorsLayer::permissive()))
}

pub async fn serve_markdown(
    base_dir: PathBuf,
    tracked_files: Vec<PathBuf>,
    is_directory_mode: bool,
    hostname: impl AsRef<str>,
    port: u16,
) -> Result<()> {
    let hostname = hostname.as_ref();

    let first_file = tracked_files.first().cloned();
    let router = new_router(base_dir.clone(), tracked_files, is_directory_mode)?;

    let listener = TcpListener::bind((hostname, port)).await?;

    let listen_addr = format_host(hostname, port);

    if is_directory_mode {
        println!("📁 Serving markdown files from: {}", base_dir.display());
    } else if let Some(file_path) = first_file {
        println!("📄 Serving markdown file: {}", file_path.display());
    }

    println!("🌐 Server running at: http://{listen_addr}");
    println!("⚡ Live reload enabled");
    println!("\nPress Ctrl+C to stop the server");

    axum::serve(listener, router).await?;

    Ok(())
}

fn format_host(hostname: &str, port: u16) -> String {
    if hostname.parse::<Ipv6Addr>().is_ok() {
        format!("[{hostname}]:{port}")
    } else {
        format!("{hostname}:{port}")
    }
}

async fn api_get_files(State(state): State<SharedMarkdownState>) -> Json<FilesResponse> {
    let state = state.lock().await;
    let files = state
        .get_sorted_filenames()
        .into_iter()
        .map(|path| ApiFile { path })
        .collect();

    Json(FilesResponse { files })
}

async fn api_get_file(
    AxumPath(path): AxumPath<String>,
    State(state): State<SharedMarkdownState>,
) -> impl IntoResponse {
    let mut state = state.lock().await;

    if !state.tracked_files.contains_key(&path) {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "File not found"})),
        )
            .into_response();
    }

    let _ = state.refresh_file(&path);

    if let Some(tracked) = state.tracked_files.get(&path) {
        Json(FileContentResponse {
            markdown: tracked.markdown.clone(),
        })
        .into_response()
    } else {
        (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "File not found"})),
        )
            .into_response()
    }
}

async fn api_update_file(
    AxumPath(path): AxumPath<String>,
    State(state): State<SharedMarkdownState>,
    Json(request): Json<FileUpdateRequest>,
) -> impl IntoResponse {
    let mut state = state.lock().await;

    match state.update_file(&path, &request.markdown) {
        Ok(()) => (StatusCode::OK, Json(serde_json::json!({"success": true}))).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

async fn api_serve_static(
    AxumPath(path): AxumPath<String>,
    State(state): State<SharedMarkdownState>,
) -> impl IntoResponse {
    // Only serve image files for security
    if !is_image_file(&path) {
        return (
            StatusCode::NOT_FOUND,
            [(header::CONTENT_TYPE, "text/plain")],
            "File not found".to_string(),
        )
            .into_response();
    }

    let state = state.lock().await;

    let full_path = state.base_dir.join(&path);

    match full_path.canonicalize() {
        Ok(canonical_path) => {
            if !canonical_path.starts_with(&state.base_dir) {
                return (
                    StatusCode::FORBIDDEN,
                    [(header::CONTENT_TYPE, "text/plain")],
                    "Access denied".to_string(),
                )
                    .into_response();
            }

            match fs::read(&canonical_path) {
                Ok(contents) => {
                    let content_type = guess_image_content_type(&path);
                    (
                        StatusCode::OK,
                        [(header::CONTENT_TYPE, content_type.as_str())],
                        contents,
                    )
                        .into_response()
                }
                Err(_) => (
                    StatusCode::NOT_FOUND,
                    [(header::CONTENT_TYPE, "text/plain")],
                    "File not found".to_string(),
                )
                    .into_response(),
            }
        }
        Err(_) => (
            StatusCode::NOT_FOUND,
            [(header::CONTENT_TYPE, "text/plain")],
            "File not found".to_string(),
        )
            .into_response(),
    }
}

async fn server_health() -> impl IntoResponse {
    (StatusCode::OK, "ready")
}

fn is_image_file(file_path: &str) -> bool {
    let extension = std::path::Path::new(file_path)
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("");

    matches!(
        extension.to_lowercase().as_str(),
        "png" | "jpg" | "jpeg" | "gif" | "svg" | "webp" | "bmp" | "ico"
    )
}

fn guess_image_content_type(file_path: &str) -> String {
    let extension = std::path::Path::new(file_path)
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("");

    match extension.to_lowercase().as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "svg" => "image/svg+xml",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        "ico" => "image/x-icon",
        _ => "application/octet-stream",
    }
    .to_string()
}

async fn websocket_handler(
    ws: WebSocketUpgrade,
    State(state): State<SharedMarkdownState>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_websocket(socket, state))
}

async fn handle_websocket(socket: WebSocket, state: SharedMarkdownState) {
    let (mut sender, mut receiver) = socket.split();

    let mut change_rx = {
        let state = state.lock().await;
        state.change_tx.subscribe()
    };

    let recv_task = tokio::spawn(async move {
        while let Some(msg) = receiver.next().await {
            match msg {
                Ok(Message::Text(text)) => {
                    if let Ok(client_msg) = serde_json::from_str::<ClientMessage>(&text) {
                        match client_msg {
                            ClientMessage::Ping | ClientMessage::RequestRefresh => {}
                        }
                    }
                }
                Ok(Message::Close(_)) => break,
                _ => {}
            }
        }
    });

    let send_task = tokio::spawn(async move {
        while let Ok(reload_msg) = change_rx.recv().await {
            if let Ok(json) = serde_json::to_string(&reload_msg) {
                if sender.send(Message::Text(json)).await.is_err() {
                    break;
                }
            }
        }
    });

    tokio::select! {
        _ = recv_task => {},
        _ = send_task => {},
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_is_markdown_file() {
        assert!(is_markdown_file(Path::new("test.md")));
        assert!(is_markdown_file(Path::new("/path/to/file.md")));
        assert!(is_markdown_file(Path::new("test.markdown")));
        assert!(is_markdown_file(Path::new("test.MD")));
        assert!(!is_markdown_file(Path::new("test.txt")));
    }

    #[test]
    fn test_is_image_file() {
        assert!(is_image_file("test.png"));
        assert!(is_image_file("test.jpg"));
        assert!(!is_image_file("test.txt"));
    }

    #[test]
    fn test_format_host() {
        assert_eq!(format_host("127.0.0.1", 3000), "127.0.0.1:3000");
        assert_eq!(format_host("::1", 3000), "[::1]:3000");
    }

    #[test]
    fn test_scan_markdown_files_empty_directory() {
        let temp_dir = tempdir().expect("Failed to create temp dir");
        let result = scan_markdown_files(temp_dir.path()).expect("Failed to scan");
        assert_eq!(result.len(), 0);
    }

    #[test]
    fn test_scan_markdown_files_with_markdown_files() {
        let temp_dir = tempdir().expect("Failed to create temp dir");

        fs::write(temp_dir.path().join("test1.md"), "# Test 1").expect("Failed to write");
        fs::write(temp_dir.path().join("test2.markdown"), "# Test 2").expect("Failed to write");
        fs::write(temp_dir.path().join("test.txt"), "text").expect("Failed to write");

        let result = scan_markdown_files(temp_dir.path()).expect("Failed to scan");
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn test_markdown_state_add_tracked_file() {
        let temp_dir = tempdir().expect("Failed to create temp dir");
        let base_dir = temp_dir
            .path()
            .canonicalize()
            .expect("Failed to canonicalize");
        let file_path = base_dir.join("test.md");
        fs::write(&file_path, "# Test").expect("Failed to write");

        let mut state = MarkdownState::new(base_dir.clone(), vec![file_path.clone()], true)
            .expect("Failed to create state");

        let new_file = base_dir.join("new.md");
        fs::write(&new_file, "# New").expect("Failed to write");

        state
            .add_tracked_file(new_file.clone())
            .expect("Failed to add file");

        let relative_path = "new.md";
        assert!(state.tracked_files.contains_key(relative_path));
        assert_eq!(
            state.tracked_files.get(relative_path).unwrap().markdown,
            "# New"
        );
    }

    #[test]
    fn test_markdown_state_add_tracked_file_duplicate() {
        let temp_dir = tempdir().expect("Failed to create temp dir");
        let base_dir = temp_dir
            .path()
            .canonicalize()
            .expect("Failed to canonicalize");
        let file_path = base_dir.join("test.md");
        fs::write(&file_path, "# Test").expect("Failed to write");

        let mut state = MarkdownState::new(base_dir.clone(), vec![file_path.clone()], true)
            .expect("Failed to create state");

        // Adding the same file again should succeed but not duplicate
        state
            .add_tracked_file(file_path.clone())
            .expect("Failed to add file");

        assert_eq!(state.tracked_files.len(), 1);
    }

    #[test]
    fn test_markdown_state_update_file() {
        let temp_dir = tempdir().expect("Failed to create temp dir");
        let base_dir = temp_dir
            .path()
            .canonicalize()
            .expect("Failed to canonicalize");
        let file_path = base_dir.join("test.md");
        fs::write(&file_path, "# Test").expect("Failed to write");

        let mut state = MarkdownState::new(base_dir.clone(), vec![file_path.clone()], false)
            .expect("Failed to create state");

        state
            .update_file("test.md", "# Updated")
            .expect("Failed to update");

        let content = fs::read_to_string(&file_path).expect("Failed to read");
        assert_eq!(content, "# Updated");
        assert_eq!(
            state.tracked_files.get("test.md").unwrap().markdown,
            "# Updated"
        );
    }

    #[test]
    fn test_markdown_state_update_file_not_found() {
        let temp_dir = tempdir().expect("Failed to create temp dir");
        let base_dir = temp_dir
            .path()
            .canonicalize()
            .expect("Failed to canonicalize");
        let file_path = base_dir.join("test.md");
        fs::write(&file_path, "# Test").expect("Failed to write");

        let mut state = MarkdownState::new(base_dir.clone(), vec![file_path], false)
            .expect("Failed to create state");

        let result = state.update_file("nonexistent.md", "# New");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("File not found"));
    }

    #[test]
    fn test_markdown_state_refresh_file() {
        let temp_dir = tempdir().expect("Failed to create temp dir");
        let base_dir = temp_dir
            .path()
            .canonicalize()
            .expect("Failed to canonicalize");
        let file_path = base_dir.join("test.md");
        fs::write(&file_path, "# Test").expect("Failed to write");

        let mut state = MarkdownState::new(base_dir.clone(), vec![file_path.clone()], false)
            .expect("Failed to create state");

        // Modify the file externally
        std::thread::sleep(std::time::Duration::from_millis(10));
        fs::write(&file_path, "# Modified").expect("Failed to write");

        state.refresh_file("test.md").expect("Failed to refresh");

        assert_eq!(
            state.tracked_files.get("test.md").unwrap().markdown,
            "# Modified"
        );
    }

    #[test]
    fn test_markdown_state_rescan_directory() {
        let temp_dir = tempdir().expect("Failed to create temp dir");
        let base_dir = temp_dir
            .path()
            .canonicalize()
            .expect("Failed to canonicalize");
        let file_path = base_dir.join("test.md");
        fs::write(&file_path, "# Test").expect("Failed to write");

        let mut state = MarkdownState::new(base_dir.clone(), vec![file_path], true)
            .expect("Failed to create state");

        // Add a new file
        let new_file = base_dir.join("new.md");
        fs::write(&new_file, "# New").expect("Failed to write");

        let changed = state.rescan_directory().expect("Failed to rescan");
        assert!(changed);
        assert_eq!(state.tracked_files.len(), 2);
        assert!(state.tracked_files.contains_key("new.md"));
    }

    #[test]
    fn test_markdown_state_rescan_directory_no_changes() {
        let temp_dir = tempdir().expect("Failed to create temp dir");
        let base_dir = temp_dir
            .path()
            .canonicalize()
            .expect("Failed to canonicalize");
        let file_path = base_dir.join("test.md");
        fs::write(&file_path, "# Test").expect("Failed to write");

        let mut state = MarkdownState::new(base_dir.clone(), vec![file_path], true)
            .expect("Failed to create state");

        let changed = state.rescan_directory().expect("Failed to rescan");
        assert!(!changed);
    }

    #[test]
    fn test_markdown_state_rescan_directory_single_file_mode() {
        let temp_dir = tempdir().expect("Failed to create temp dir");
        let base_dir = temp_dir
            .path()
            .canonicalize()
            .expect("Failed to canonicalize");
        let file_path = base_dir.join("test.md");
        fs::write(&file_path, "# Test").expect("Failed to write");

        let mut state = MarkdownState::new(base_dir.clone(), vec![file_path], false)
            .expect("Failed to create state");

        let changed = state.rescan_directory().expect("Failed to rescan");
        assert!(!changed);
    }

    #[test]
    fn test_markdown_state_rescan_directory_file_removed() {
        let temp_dir = tempdir().expect("Failed to create temp dir");
        let base_dir = temp_dir
            .path()
            .canonicalize()
            .expect("Failed to canonicalize");
        let file1 = base_dir.join("test1.md");
        let file2 = base_dir.join("test2.md");
        fs::write(&file1, "# Test 1").expect("Failed to write");
        fs::write(&file2, "# Test 2").expect("Failed to write");

        let mut state =
            MarkdownState::new(base_dir.clone(), vec![file1.clone(), file2.clone()], true)
                .expect("Failed to create state");

        assert_eq!(state.tracked_files.len(), 2);

        // Remove one file
        fs::remove_file(&file2).expect("Failed to remove");

        let changed = state.rescan_directory().expect("Failed to rescan");
        assert!(changed);
        assert_eq!(state.tracked_files.len(), 1);
        assert!(state.tracked_files.contains_key("test1.md"));
        assert!(!state.tracked_files.contains_key("test2.md"));
    }

    #[test]
    fn test_detect_file_change_rename() {
        use std::collections::{HashMap, HashSet};

        let mut old_files = HashSet::new();
        old_files.insert("old.md".to_string());

        let mut new_files = HashSet::new();
        new_files.insert("new.md".to_string());

        let mut old_tracked = HashMap::new();
        let hash = md5::compute(b"content");
        old_tracked.insert("old.md".to_string(), hash);

        let mut new_tracked = HashMap::new();
        new_tracked.insert(
            "new.md".to_string(),
            TrackedFile {
                path: PathBuf::from("new.md"),
                last_modified: SystemTime::now(),
                markdown: "content".to_string(),
                content_hash: hash,
            },
        );

        match detect_file_change(&old_files, &new_files, &old_tracked, &new_tracked) {
            FileChangeType::Renamed { old_name, new_name } => {
                assert_eq!(old_name, "old.md");
                assert_eq!(new_name, "new.md");
            }
            _ => panic!("Expected Renamed"),
        }
    }

    #[test]
    fn test_detect_file_change_removed() {
        use std::collections::{HashMap, HashSet};

        let mut old_files = HashSet::new();
        old_files.insert("removed.md".to_string());

        let new_files = HashSet::new();
        let old_tracked = HashMap::new();
        let new_tracked = HashMap::new();

        match detect_file_change(&old_files, &new_files, &old_tracked, &new_tracked) {
            FileChangeType::Removed { name } => {
                assert_eq!(name, "removed.md");
            }
            _ => panic!("Expected Removed"),
        }
    }

    #[test]
    fn test_detect_file_change_added() {
        use std::collections::{HashMap, HashSet};

        let mut old_files = HashSet::new();
        old_files.insert("file1.md".to_string());

        let mut new_files = HashSet::new();
        new_files.insert("file1.md".to_string());
        new_files.insert("file2.md".to_string());

        let old_tracked = HashMap::new();
        let new_tracked = HashMap::new();

        match detect_file_change(&old_files, &new_files, &old_tracked, &new_tracked) {
            FileChangeType::Added { name } => {
                assert_eq!(name, "file2.md");
            }
            _ => panic!("Expected Added"),
        }
    }

    #[test]
    fn test_send_change_message_renamed() {
        let (tx, mut rx) = broadcast::channel(10);

        send_change_message(
            FileChangeType::Renamed {
                old_name: "old.md".to_string(),
                new_name: "new.md".to_string(),
            },
            &tx,
        );

        match rx.try_recv() {
            Ok(ServerMessage::FileRenamed { old_name, new_name }) => {
                assert_eq!(old_name, "old.md");
                assert_eq!(new_name, "new.md");
            }
            _ => panic!("Expected FileRenamed message"),
        }
    }

    #[test]
    fn test_send_change_message_removed() {
        let (tx, mut rx) = broadcast::channel(10);

        send_change_message(
            FileChangeType::Removed {
                name: "removed.md".to_string(),
            },
            &tx,
        );

        match rx.try_recv() {
            Ok(ServerMessage::FileRemoved { name }) => {
                assert_eq!(name, "removed.md");
            }
            _ => panic!("Expected FileRemoved message"),
        }
    }

    #[test]
    fn test_send_change_message_added() {
        let (tx, mut rx) = broadcast::channel(10);

        send_change_message(
            FileChangeType::Added {
                name: "added.md".to_string(),
            },
            &tx,
        );

        match rx.try_recv() {
            Ok(ServerMessage::FileAdded { name }) => {
                assert_eq!(name, "added.md");
            }
            _ => panic!("Expected FileAdded message"),
        }
    }

    #[test]
    fn test_send_change_message_reload() {
        let (tx, mut rx) = broadcast::channel(10);

        send_change_message(FileChangeType::Other, &tx);

        match rx.try_recv() {
            Ok(ServerMessage::Reload) => {}
            _ => panic!("Expected Reload message"),
        }
    }

    #[test]
    fn test_is_image_file_all_types() {
        // Test all supported image types
        assert!(is_image_file("test.png"));
        assert!(is_image_file("test.PNG"));
        assert!(is_image_file("test.jpg"));
        assert!(is_image_file("test.jpeg"));
        assert!(is_image_file("test.gif"));
        assert!(is_image_file("test.svg"));
        assert!(is_image_file("test.webp"));
        assert!(is_image_file("test.bmp"));
        assert!(is_image_file("test.ico"));
        // Test non-image files
        assert!(!is_image_file("test.txt"));
        assert!(!is_image_file("test.md"));
        assert!(!is_image_file("test"));
    }

    #[test]
    fn test_guess_image_content_type() {
        assert_eq!(guess_image_content_type("test.png"), "image/png");
        assert_eq!(guess_image_content_type("test.jpg"), "image/jpeg");
        assert_eq!(guess_image_content_type("test.jpeg"), "image/jpeg");
        assert_eq!(guess_image_content_type("test.gif"), "image/gif");
        assert_eq!(guess_image_content_type("test.svg"), "image/svg+xml");
        assert_eq!(guess_image_content_type("test.webp"), "image/webp");
        assert_eq!(guess_image_content_type("test.bmp"), "image/bmp");
        assert_eq!(guess_image_content_type("test.ico"), "image/x-icon");
        assert_eq!(
            guess_image_content_type("test.txt"),
            "application/octet-stream"
        );
    }

    #[test]
    fn test_markdown_state_get_sorted_filenames() {
        let temp_dir = tempdir().expect("Failed to create temp dir");
        let base_dir = temp_dir
            .path()
            .canonicalize()
            .expect("Failed to canonicalize");

        let file1 = base_dir.join("b.md");
        let file2 = base_dir.join("a.md");
        let file3 = base_dir.join("c.md");

        fs::write(&file1, "# B").expect("Failed to write");
        fs::write(&file2, "# A").expect("Failed to write");
        fs::write(&file3, "# C").expect("Failed to write");

        let state = MarkdownState::new(base_dir.clone(), vec![file1, file2, file3], true)
            .expect("Failed to create state");

        let sorted = state.get_sorted_filenames();
        assert_eq!(sorted, vec!["a.md", "b.md", "c.md"]);
    }

    #[test]
    fn test_calculate_relative_path() {
        let temp_dir = tempdir().expect("Failed to create temp dir");
        let base_dir = temp_dir
            .path()
            .canonicalize()
            .expect("Failed to canonicalize");
        let file_path = base_dir.join("test.md");

        fs::write(&file_path, "# Test").expect("Failed to write");

        let relative = calculate_relative_path(&file_path, &base_dir)
            .expect("Failed to calculate relative path");
        assert_eq!(relative, "test.md");
    }

    #[test]
    fn test_calculate_relative_path_nested() {
        let temp_dir = tempdir().expect("Failed to create temp dir");
        let base_dir = temp_dir
            .path()
            .canonicalize()
            .expect("Failed to canonicalize");
        let nested_dir = base_dir.join("nested");
        fs::create_dir(&nested_dir).expect("Failed to create nested dir");
        let file_path = nested_dir.join("test.md");

        fs::write(&file_path, "# Test").expect("Failed to write");

        let relative = calculate_relative_path(&file_path, &base_dir)
            .expect("Failed to calculate relative path");
        assert_eq!(relative, "nested/test.md");
    }

    #[test]
    fn test_scan_markdown_files_nested() {
        let temp_dir = tempdir().expect("Failed to create temp dir");
        let nested_dir = temp_dir.path().join("nested");
        fs::create_dir(&nested_dir).expect("Failed to create nested dir");

        fs::write(temp_dir.path().join("root.md"), "# Root").expect("Failed to write");
        fs::write(nested_dir.join("nested.md"), "# Nested").expect("Failed to write");

        let result = scan_markdown_files(temp_dir.path()).expect("Failed to scan");
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn test_markdown_state_refresh_file_not_modified() {
        let temp_dir = tempdir().expect("Failed to create temp dir");
        let base_dir = temp_dir
            .path()
            .canonicalize()
            .expect("Failed to canonicalize");
        let file_path = base_dir.join("test.md");

        fs::write(&file_path, "# Test").expect("Failed to write");

        let mut state = MarkdownState::new(base_dir.clone(), vec![file_path.clone()], false)
            .expect("Failed to create state");

        // Get original content
        let original_content = state.tracked_files.get("test.md").unwrap().markdown.clone();

        // Refresh without modification
        state.refresh_file("test.md").expect("Failed to refresh");

        // Content should be unchanged
        assert_eq!(
            state.tracked_files.get("test.md").unwrap().markdown,
            original_content
        );
    }

    #[test]
    fn test_format_host_ipv4() {
        assert_eq!(format_host("0.0.0.0", 8080), "0.0.0.0:8080");
        assert_eq!(format_host("192.168.1.1", 3000), "192.168.1.1:3000");
    }

    #[test]
    fn test_format_host_ipv6() {
        assert_eq!(format_host("::1", 3000), "[::1]:3000");
        assert_eq!(format_host("fe80::1", 8080), "[fe80::1]:8080");
    }
}
