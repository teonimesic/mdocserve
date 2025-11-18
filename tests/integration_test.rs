use axum_test::TestServer;
use mdocserve::{new_router, scan_markdown_files, ClientMessage, ServerMessage};
use std::fs;
use std::path::PathBuf;
use std::time::Duration;
use tempfile::{tempdir, TempDir};

const WEBSOCKET_TIMEOUT_SECS: u64 = 5;

const TEST_FILE_1_CONTENT: &str = "# Test 1\n\nContent of test1";
const TEST_FILE_2_CONTENT: &str = "# Test 2\n\nContent of test2";
const TEST_FILE_3_CONTENT: &str = "# Test 3\n\nContent of test3";

// Holds test server and temp directory to keep files alive
struct TestSetup {
    server: TestServer,
    _temp_dir: TempDir,
    file_path: PathBuf,
}

fn create_test_server_impl(content: &str, use_http: bool) -> TestSetup {
    // Create a dedicated temp directory to avoid permission issues with /tmp siblings in CI
    let temp_dir = tempdir().expect("Failed to create temp dir");
    let temp_file_path = temp_dir.path().join("test.md");
    fs::write(&temp_file_path, content).expect("Failed to write temp file");

    // Use the temp directory as base_dir instead of /tmp
    let base_dir = temp_dir.path().to_path_buf();
    let tracked_files = vec![temp_file_path.clone()];
    let is_directory_mode = false;

    let router =
        new_router(base_dir, tracked_files, is_directory_mode).expect("Failed to create router");

    let server = if use_http {
        TestServer::builder()
            .http_transport()
            .build(router)
            .expect("Failed to create test server")
    } else {
        TestServer::new(router).expect("Failed to create test server")
    };

    TestSetup {
        server,
        _temp_dir: temp_dir,
        file_path: temp_file_path,
    }
}

async fn create_test_server(content: &str) -> TestSetup {
    create_test_server_impl(content, false)
}

async fn create_test_server_with_http(content: &str) -> TestSetup {
    create_test_server_impl(content, true)
}

fn create_directory_server_impl(use_http: bool) -> (TestServer, TempDir) {
    let temp_dir = tempdir().expect("Failed to create temp dir");

    fs::write(temp_dir.path().join("test1.md"), TEST_FILE_1_CONTENT)
        .expect("Failed to write test1.md");
    fs::write(temp_dir.path().join("test2.markdown"), TEST_FILE_2_CONTENT)
        .expect("Failed to write test2.markdown");
    fs::write(temp_dir.path().join("test3.md"), TEST_FILE_3_CONTENT)
        .expect("Failed to write test3.md");

    let base_dir = temp_dir.path().to_path_buf();
    let tracked_files = scan_markdown_files(&base_dir).expect("Failed to scan markdown files");
    let is_directory_mode = true;

    let router =
        new_router(base_dir, tracked_files, is_directory_mode).expect("Failed to create router");

    let server = if use_http {
        TestServer::builder()
            .http_transport()
            .build(router)
            .expect("Failed to create test server")
    } else {
        TestServer::new(router).expect("Failed to create test server")
    };

    (server, temp_dir)
}

async fn create_directory_server() -> (TestServer, TempDir) {
    create_directory_server_impl(false)
}

async fn create_directory_server_with_http() -> (TestServer, TempDir) {
    create_directory_server_impl(true)
}

#[tokio::test]
async fn test_websocket_connection() {
    let setup = create_test_server_with_http("# WebSocket Test").await;

    // Test that WebSocket endpoint exists and can be connected to
    let response = setup.server.get_websocket("/ws").await;
    response.assert_status_switching_protocols();
}

#[tokio::test]
async fn test_file_modification_updates_via_websocket() {
    let setup = create_test_server_with_http("# Original Content").await;

    let mut websocket = setup
        .server
        .get_websocket("/ws")
        .await
        .into_websocket()
        .await;

    // Modify the file
    fs::write(&setup.file_path, "# Modified Content").expect("Failed to modify file");

    // Should receive reload signal via WebSocket (with timeout)
    let update_result = tokio::time::timeout(
        Duration::from_secs(WEBSOCKET_TIMEOUT_SECS),
        websocket.receive_json::<ServerMessage>(),
    )
    .await;

    match update_result {
        Ok(update_message) => {
            if let ServerMessage::FileModified { name } = update_message {
                assert_eq!(
                    name, "test.md",
                    "FileModified should reference the correct file"
                );
            } else {
                panic!(
                    "Expected FileModified message after file modification, got {:?}",
                    update_message
                );
            }
        }
        Err(_) => {
            panic!("Timeout waiting for WebSocket update after file modification");
        }
    }
}

#[tokio::test]
async fn test_unknown_routes_serve_spa() {
    let setup = create_test_server("# SPA Test").await;

    // With embedded frontend, unknown routes serve the SPA for client-side routing
    let response = setup.server.get("/unknown-route").await;
    assert_eq!(response.status_code(), 200);

    let html = response.text();
    assert!(html.contains("<!doctype html>"), "Should serve SPA HTML");
}

#[tokio::test]
async fn test_non_image_files_not_served_via_api() {
    use tempfile::tempdir;

    // Create a temporary directory
    let temp_dir = tempdir().expect("Failed to create temp dir");

    // Create a markdown file
    let md_content = "# Test";
    let md_path = temp_dir.path().join("test.md");
    fs::write(&md_path, md_content).expect("Failed to write markdown file");

    // Create a non-image file (txt)
    let txt_path = temp_dir.path().join("secret.txt");
    fs::write(&txt_path, "secret content").expect("Failed to write txt file");

    // Create router with the markdown file (single-file mode)
    let base_dir = temp_dir.path().to_path_buf();
    let tracked_files = vec![md_path];
    let is_directory_mode = false;
    let router =
        new_router(base_dir, tracked_files, is_directory_mode).expect("Failed to create router");
    let server = TestServer::new(router).expect("Failed to create test server");

    // Test that non-image files return 404 via API static endpoint
    let response = server.get("/api/static/secret.txt").await;
    assert_eq!(
        response.status_code(),
        404,
        "Secret files should not be accessible via API"
    );

    // Accessing non-API routes serves the SPA
    let response = server.get("/secret.txt").await;
    assert_eq!(response.status_code(), 200, "Non-API routes serve the SPA");
    assert!(
        response.text().contains("<!doctype html>"),
        "Should serve SPA HTML, not file content"
    );
}

// Directory mode tests

#[tokio::test]
async fn test_directory_mode_non_api_routes_serve_spa() {
    let (server, _temp_dir) = create_directory_server().await;

    // Non-API routes (even with .md extension) serve the SPA for client-side routing
    let response = server.get("/nonexistent.md").await;
    assert_eq!(response.status_code(), 200, "Non-API routes serve the SPA");

    let html = response.text();
    assert!(html.contains("<!doctype html>"), "Should serve SPA HTML");
}

#[tokio::test]
async fn test_single_file_mode_no_navigation_sidebar() {
    let setup = create_test_server("# Single File Test").await;

    let response = setup.server.get("/").await;
    assert_eq!(response.status_code(), 200);
    let body = response.text();

    // Verify no navigation sidebar in single-file mode
    assert!(!body.contains(r#"<nav class="sidebar">"#));
    assert!(!body.contains("<h3>Files</h3>"));
    assert!(!body.contains(r#"<ul class="file-list">"#));
}

#[tokio::test]
async fn test_directory_mode_websocket_file_modification() {
    let (server, temp_dir) = create_directory_server_with_http().await;

    let mut websocket = server.get_websocket("/ws").await.into_websocket().await;

    // Modify one of the tracked files
    let test_file = temp_dir.path().join("test1.md");
    fs::write(&test_file, "# Modified Test 1\n\nContent has changed")
        .expect("Failed to modify file");

    // Should receive reload signal via WebSocket
    let update_result = tokio::time::timeout(
        Duration::from_secs(WEBSOCKET_TIMEOUT_SECS),
        websocket.receive_json::<ServerMessage>(),
    )
    .await;

    match update_result {
        Ok(update_message) => {
            if let ServerMessage::FileModified { name } = update_message {
                assert_eq!(
                    name, "test1.md",
                    "FileModified should reference the correct file"
                );
            } else {
                panic!(
                    "Expected FileModified message after file modification, got {:?}",
                    update_message
                );
            }
        }
        Err(_) => {
            panic!("Timeout waiting for WebSocket update after file modification");
        }
    }
}

// File rename and removal tests

#[tokio::test]
async fn test_folder_based_routing_404_for_nonexistent_path() {
    let temp_dir = tempdir().expect("Failed to create temp dir");

    let folder1 = temp_dir.path().join("folder1");
    fs::create_dir(&folder1).expect("Failed to create folder1");
    fs::write(folder1.join("doc.md"), "# Doc").expect("Failed to write file");

    let base_dir = temp_dir.path().to_path_buf();
    let tracked_files = scan_markdown_files(&base_dir).expect("Failed to scan");

    let router = new_router(base_dir, tracked_files, true).expect("Failed to create router");
    let server = TestServer::new(router).expect("Failed to create test server");

    // Non-API routes serve the SPA, even for non-existent paths
    let response = server.get("/nonexistent/doc.md").await;
    assert_eq!(response.status_code(), 200, "Non-API routes serve the SPA");

    let html = response.text();
    assert!(html.contains("<!doctype html>"), "Should serve SPA HTML");

    // Same for non-existent files in existing folders
    let response = server.get("/folder1/nonexistent.md").await;
    assert_eq!(response.status_code(), 200, "Non-API routes serve the SPA");
    assert!(
        response.text().contains("<!doctype html>"),
        "Should serve SPA HTML"
    );
}

// ===========================
// Tree Structure Tests
// ===========================

// ===========================
// Folder Removal Tests
// ===========================

// ===========================
// Root Route Tests
// ===========================

// ============================================================================
// React API Tests - GET /api/files
// ============================================================================

#[tokio::test]
async fn test_api_get_files_single_file() {
    let (server, _temp_dir) = create_directory_server().await;

    let response = server.get("/api/files").await;

    assert_eq!(response.status_code(), 200);

    let json = response.json::<serde_json::Value>();
    let files = json["files"].as_array().expect("files should be an array");

    assert_eq!(files.len(), 3);
    assert_eq!(files[0]["path"], "test1.md");
    assert_eq!(files[1]["path"], "test2.markdown");
    assert_eq!(files[2]["path"], "test3.md");
}

#[tokio::test]
async fn test_api_get_files_nested_folders() {
    let temp_dir = tempdir().expect("Failed to create temp dir");

    // Create folder structure
    let folder1 = temp_dir.path().join("folder1");
    fs::create_dir(&folder1).expect("Failed to create folder1");
    fs::write(folder1.join("file1.md"), "# File 1").expect("Failed to write file1");

    let folder2 = temp_dir.path().join("folder2");
    fs::create_dir(&folder2).expect("Failed to create folder2");
    fs::write(folder2.join("file2.md"), "# File 2").expect("Failed to write file2");

    fs::write(temp_dir.path().join("root.md"), "# Root").expect("Failed to write root");

    let base_dir = temp_dir.path().to_path_buf();
    let tracked_files = scan_markdown_files(&base_dir).expect("Failed to scan");
    let router = new_router(base_dir, tracked_files, true).expect("Failed to create router");
    let server = TestServer::new(router).expect("Failed to create server");

    let response = server.get("/api/files").await;

    assert_eq!(response.status_code(), 200);

    let json = response.json::<serde_json::Value>();
    let files = json["files"].as_array().expect("files should be an array");

    assert_eq!(files.len(), 3);

    // Check file paths
    let paths: Vec<String> = files
        .iter()
        .map(|f| f["path"].as_str().unwrap().to_string())
        .collect();

    assert!(
        paths.contains(&"folder1/file1.md".to_string())
            || paths.contains(&"folder1\\file1.md".to_string())
    );
    assert!(
        paths.contains(&"folder2/file2.md".to_string())
            || paths.contains(&"folder2\\file2.md".to_string())
    );
    assert!(paths.contains(&"root.md".to_string()));
}

// ============================================================================
// React API Tests - GET /api/files/:path
// ============================================================================

#[tokio::test]
async fn test_api_get_file_returns_markdown_content() {
    let (server, _temp_dir) = create_directory_server().await;

    let response = server.get("/api/files/test1.md").await;

    assert_eq!(response.status_code(), 200);

    let json = response.json::<serde_json::Value>();
    assert_eq!(json["markdown"], TEST_FILE_1_CONTENT);
}

#[tokio::test]
async fn test_api_get_file_nested_folder() {
    let temp_dir = tempdir().expect("Failed to create temp dir");

    let folder1 = temp_dir.path().join("folder1");
    fs::create_dir(&folder1).expect("Failed to create folder1");
    fs::write(folder1.join("nested.md"), "# Nested File").expect("Failed to write nested");

    let base_dir = temp_dir.path().to_path_buf();
    let tracked_files = scan_markdown_files(&base_dir).expect("Failed to scan");
    let router = new_router(base_dir, tracked_files, true).expect("Failed to create router");
    let server = TestServer::new(router).expect("Failed to create server");

    // Try both path separators
    let response = server.get("/api/files/folder1/nested.md").await;

    if response.status_code() != 200 {
        // Try Windows-style path
        let response = server.get("/api/files/folder1\\nested.md").await;
        assert_eq!(response.status_code(), 200);
        let json = response.json::<serde_json::Value>();
        assert_eq!(json["markdown"], "# Nested File");
    } else {
        let json = response.json::<serde_json::Value>();
        assert_eq!(json["markdown"], "# Nested File");
    }
}

#[tokio::test]
async fn test_api_get_file_not_found() {
    let (server, _temp_dir) = create_directory_server().await;

    let response = server.get("/api/files/nonexistent.md").await;

    assert_eq!(response.status_code(), 404);
}

// ============================================================================
// React API Tests - PUT /api/files/:path
// ============================================================================

#[tokio::test]
async fn test_api_update_file_success() {
    let (server, temp_dir) = create_directory_server().await;

    let update_payload = serde_json::json!({
        "markdown": "# Updated Content\n\nThis is new content"
    });

    let response = server
        .put("/api/files/test1.md")
        .json(&update_payload)
        .await;

    assert_eq!(response.status_code(), 200);

    let json = response.json::<serde_json::Value>();
    assert_eq!(json["success"], true);

    // Verify file was actually updated on disk
    let file_content =
        fs::read_to_string(temp_dir.path().join("test1.md")).expect("Failed to read updated file");
    assert_eq!(file_content, "# Updated Content\n\nThis is new content");
}

#[tokio::test]
async fn test_api_update_file_checkbox() {
    let temp_dir = tempdir().expect("Failed to create temp dir");

    let checkbox_content = "# Todo List\n\n- [ ] Task 1\n- [ ] Task 2\n- [ ] Task 3";
    fs::write(temp_dir.path().join("todo.md"), checkbox_content).expect("Failed to write todo");

    let base_dir = temp_dir.path().to_path_buf();
    let tracked_files = scan_markdown_files(&base_dir).expect("Failed to scan");
    let router =
        new_router(base_dir.clone(), tracked_files, true).expect("Failed to create router");
    let server = TestServer::new(router).expect("Failed to create server");

    // Update first checkbox to checked
    let updated_content = "# Todo List\n\n- [x] Task 1\n- [ ] Task 2\n- [ ] Task 3";
    let update_payload = serde_json::json!({
        "markdown": updated_content
    });

    let response = server.put("/api/files/todo.md").json(&update_payload).await;

    assert_eq!(response.status_code(), 200);

    // Verify file was updated
    let file_content =
        fs::read_to_string(temp_dir.path().join("todo.md")).expect("Failed to read updated file");
    assert_eq!(file_content, updated_content);
}

#[tokio::test]
async fn test_api_update_file_triggers_websocket_reload() {
    let (server, temp_dir) = create_directory_server_with_http().await;

    let mut websocket = server.get_websocket("/ws").await.into_websocket().await;

    // Update file via API
    let update_payload = serde_json::json!({
        "markdown": "# Updated via API"
    });

    let response = server
        .put("/api/files/test1.md")
        .json(&update_payload)
        .await;

    assert_eq!(response.status_code(), 200);

    // Should receive reload signal via WebSocket
    let update_result = tokio::time::timeout(
        Duration::from_secs(WEBSOCKET_TIMEOUT_SECS),
        websocket.receive_json::<ServerMessage>(),
    )
    .await;

    match update_result {
        Ok(update_message) => {
            assert_eq!(update_message, ServerMessage::Reload);
        }
        Err(_) => {
            panic!("Timeout waiting for WebSocket reload after API update");
        }
    }

    // Verify file was actually updated
    let file_content =
        fs::read_to_string(temp_dir.path().join("test1.md")).expect("Failed to read updated file");
    assert_eq!(file_content, "# Updated via API");
}

#[tokio::test]
async fn test_api_update_file_not_found() {
    let (server, _temp_dir) = create_directory_server().await;

    let update_payload = serde_json::json!({
        "markdown": "# New Content"
    });

    let response = server
        .put("/api/files/nonexistent.md")
        .json(&update_payload)
        .await;

    assert_eq!(response.status_code(), 500);

    let json = response.json::<serde_json::Value>();
    assert!(json["error"].as_str().unwrap().contains("not found"));
}

#[tokio::test]
async fn test_frontend_served_from_embedded_assets() {
    let (server, _temp_dir) = create_directory_server().await;

    // Test that the root path serves the frontend index.html
    let response = server.get("/").await;
    assert_eq!(response.status_code(), 200);

    let html = response.text();
    // Verify it's HTML and not an error message
    assert!(
        html.contains("<!doctype html>"),
        "Should serve HTML, not an error"
    );
    assert!(
        html.contains("<div id=\"root\"></div>"),
        "Should contain React root element"
    );
    assert!(
        !html.contains("Frontend not built"),
        "Should not show 'Frontend not built' error"
    );

    // Verify the HTML references CSS and JS assets
    assert!(html.contains(".css"), "Should reference CSS files");
    assert!(html.contains(".js"), "Should reference JavaScript files");
}

#[tokio::test]
async fn test_frontend_assets_accessible() {
    let (server, _temp_dir) = create_directory_server().await;

    // First get the index.html to find asset paths
    let response = server.get("/").await;
    let html = response.text();

    // Extract a CSS file path from the HTML
    // Looking for something like: href="/assets/index-ASLLX56R.css"
    let css_start = html.find("href=\"/assets/").unwrap();
    let css_path_start = css_start + 6; // Skip 'href="'
    let css_end = html[css_path_start..].find("\"").unwrap();
    let css_path = &html[css_path_start..css_path_start + css_end];

    // Test that CSS asset is accessible
    let css_response = server.get(css_path).await;
    assert_eq!(
        css_response.status_code(),
        200,
        "CSS asset should be accessible"
    );
    assert_eq!(
        css_response.header("content-type"),
        "text/css",
        "CSS should have correct MIME type"
    );

    // Extract a JS file path from the HTML
    // Looking for something like: src="/assets/index-DUnTVOz5.js"
    let js_start = html.find("src=\"/assets/").unwrap();
    let js_path_start = js_start + 5; // Skip 'src="'
    let js_end = html[js_path_start..].find("\"").unwrap();
    let js_path = &html[js_path_start..js_path_start + js_end];

    // Test that JS asset is accessible
    let js_response = server.get(js_path).await;
    assert_eq!(
        js_response.status_code(),
        200,
        "JS asset should be accessible"
    );
    assert_eq!(
        js_response.header("content-type"),
        "text/javascript",
        "JS should have correct MIME type"
    );
}

#[tokio::test]
async fn test_frontend_spa_routing() {
    let (server, _temp_dir) = create_directory_server().await;

    // Test that unknown routes serve index.html for SPA routing
    let response = server.get("/some/unknown/route").await;
    assert_eq!(response.status_code(), 200);

    let html = response.text();
    // Should serve the same index.html for SPA client-side routing
    assert!(
        html.contains("<!doctype html>"),
        "Unknown routes should serve HTML for SPA routing"
    );
    assert!(
        html.contains("<div id=\"root\"></div>"),
        "Should contain React root element"
    );
}

// ============================================================================
// Health Endpoint Tests
// ============================================================================

#[tokio::test]
async fn test_health_endpoint() {
    let (server, _temp_dir) = create_directory_server().await;

    let response = server.get("/__health").await;

    assert_eq!(response.status_code(), 200);
    assert_eq!(response.text(), "ready");
}

// ============================================================================
// API Static File Serving Tests
// ============================================================================

#[tokio::test]
async fn test_api_static_serves_image_successfully() {
    let temp_dir = tempdir().expect("Failed to create temp dir");

    // Create a test markdown file
    fs::write(temp_dir.path().join("test.md"), "# Test").expect("Failed to write markdown");

    // Create a test image file (simple PNG header)
    let png_bytes = vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]; // PNG magic bytes
    fs::write(temp_dir.path().join("test.png"), &png_bytes).expect("Failed to write image");

    let base_dir = temp_dir.path().to_path_buf();
    let tracked_files = scan_markdown_files(&base_dir).expect("Failed to scan");
    let router = new_router(base_dir, tracked_files, true).expect("Failed to create router");
    let server = TestServer::new(router).expect("Failed to create server");

    // Test that image is served successfully
    let response = server.get("/api/static/test.png").await;
    assert_eq!(response.status_code(), 200);
    assert_eq!(response.header("content-type"), "image/png");
    assert_eq!(response.as_bytes(), &png_bytes);
}

#[tokio::test]
async fn test_api_static_path_traversal_blocked() {
    use std::os::unix::fs::symlink;

    let temp_dir = tempdir().expect("Failed to create temp dir");

    // Create a markdown file in the temp dir
    fs::write(temp_dir.path().join("test.md"), "# Test").expect("Failed to write markdown");

    // Create a "secret" file outside the base directory (in parent)
    let parent_dir = temp_dir.path().parent().unwrap();
    let secret_file = parent_dir.join("secret_image.png");
    fs::write(&secret_file, "SECRET DATA").expect("Failed to write secret file");

    // Create a symlink inside base_dir that points to the secret file outside
    let symlink_path = temp_dir.path().join("link_to_secret.png");
    symlink(&secret_file, &symlink_path).expect("Failed to create symlink");

    let base_dir = temp_dir.path().to_path_buf();
    let tracked_files = scan_markdown_files(&base_dir).expect("Failed to scan");
    let router = new_router(base_dir, tracked_files, true).expect("Failed to create router");
    let server = TestServer::new(router).expect("Failed to create server");

    // Try to access file outside base directory via symlink
    let response = server.get("/api/static/link_to_secret.png").await;
    assert_eq!(
        response.status_code(),
        403,
        "Symlink to file outside base dir should be blocked with 403 Forbidden"
    );
    assert_eq!(response.text(), "Access denied");

    // Cleanup
    fs::remove_file(&symlink_path).ok();
    fs::remove_file(&secret_file).ok();
}

#[tokio::test]
async fn test_api_static_nonexistent_image_returns_404() {
    let (server, _temp_dir) = create_directory_server().await;

    // Request a non-existent image file
    let response = server.get("/api/static/nonexistent.png").await;
    assert_eq!(response.status_code(), 404);
    assert_eq!(response.text(), "File not found");
}

#[tokio::test]
async fn test_api_static_supports_multiple_image_formats() {
    let temp_dir = tempdir().expect("Failed to create temp dir");

    fs::write(temp_dir.path().join("test.md"), "# Test").expect("Failed to write markdown");

    // Create test images for different formats
    fs::write(temp_dir.path().join("test.jpg"), "JPEG data").expect("Failed to write jpg");
    fs::write(temp_dir.path().join("test.gif"), "GIF data").expect("Failed to write gif");
    fs::write(temp_dir.path().join("test.svg"), "SVG data").expect("Failed to write svg");
    fs::write(temp_dir.path().join("test.webp"), "WEBP data").expect("Failed to write webp");

    let base_dir = temp_dir.path().to_path_buf();
    let tracked_files = scan_markdown_files(&base_dir).expect("Failed to scan");
    let router = new_router(base_dir, tracked_files, true).expect("Failed to create router");
    let server = TestServer::new(router).expect("Failed to create server");

    // Test JPG
    let response = server.get("/api/static/test.jpg").await;
    assert_eq!(response.status_code(), 200);
    assert_eq!(response.header("content-type"), "image/jpeg");

    // Test GIF
    let response = server.get("/api/static/test.gif").await;
    assert_eq!(response.status_code(), 200);
    assert_eq!(response.header("content-type"), "image/gif");

    // Test SVG
    let response = server.get("/api/static/test.svg").await;
    assert_eq!(response.status_code(), 200);
    assert_eq!(response.header("content-type"), "image/svg+xml");

    // Test WebP
    let response = server.get("/api/static/test.webp").await;
    assert_eq!(response.status_code(), 200);
    assert_eq!(response.header("content-type"), "image/webp");
}

// ============================================================================
// WebSocket Message Type Tests
// ============================================================================

#[tokio::test]
async fn test_websocket_ping_message() {
    let setup = create_test_server_with_http("# Test").await;
    let mut websocket = setup
        .server
        .get_websocket("/ws")
        .await
        .into_websocket()
        .await;

    // Send Ping message
    let ping_msg = serde_json::to_string(&ClientMessage::Ping).unwrap();
    websocket.send_text(ping_msg).await;

    // Connection should remain open - verify by sending another message
    // If the ping caused an error, this would fail
    let ping_msg2 = serde_json::to_string(&ClientMessage::Ping).unwrap();
    websocket.send_text(ping_msg2).await;

    // Success - Ping messages are handled without error
}

#[tokio::test]
async fn test_websocket_request_refresh_message() {
    let setup = create_test_server_with_http("# Test").await;
    let mut websocket = setup
        .server
        .get_websocket("/ws")
        .await
        .into_websocket()
        .await;

    // Send RequestRefresh message
    let refresh_msg = serde_json::to_string(&ClientMessage::RequestRefresh).unwrap();
    websocket.send_text(refresh_msg).await;

    // Connection should remain open
    let ping_msg = serde_json::to_string(&ClientMessage::Ping).unwrap();
    websocket.send_text(ping_msg).await;

    // Success - RequestRefresh messages are handled without error
}

#[tokio::test]
async fn test_websocket_close_message() {
    let setup = create_test_server_with_http("# Test").await;
    let websocket = setup
        .server
        .get_websocket("/ws")
        .await
        .into_websocket()
        .await;

    // Send Close message
    websocket.close().await;

    // Wait a moment for the close to process
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Connection should be closed - attempting to receive should indicate closure
    // The websocket library should handle this gracefully
}

#[tokio::test]
async fn test_websocket_invalid_json() {
    let setup = create_test_server_with_http("# Test").await;
    let mut websocket = setup
        .server
        .get_websocket("/ws")
        .await
        .into_websocket()
        .await;

    // Send invalid JSON
    websocket.send_text("{invalid json}").await;

    // Connection should remain open despite invalid JSON
    // Verify by sending a valid message
    let ping_msg = serde_json::to_string(&ClientMessage::Ping).unwrap();
    websocket.send_text(ping_msg).await;

    // Success - invalid JSON doesn't crash the connection
}

// ============================================================================
// File Event Handler Tests
// ============================================================================

#[tokio::test]
async fn test_image_file_modification_triggers_reload() {
    let (server, temp_dir) = create_directory_server_with_http().await;
    let mut websocket = server.get_websocket("/ws").await.into_websocket().await;

    // Create an image file
    let image_path = temp_dir.path().join("test-image.png");
    fs::write(&image_path, vec![0x89, 0x50, 0x4E, 0x47]).expect("Failed to write image");

    // Wait for file system event to be processed
    tokio::time::sleep(Duration::from_millis(300)).await;

    // Modify the image file
    fs::write(&image_path, vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A])
        .expect("Failed to modify image");

    // Should receive Reload message via WebSocket
    let update_result = tokio::time::timeout(
        Duration::from_secs(WEBSOCKET_TIMEOUT_SECS),
        websocket.receive_json::<ServerMessage>(),
    )
    .await;

    match update_result {
        Ok(message) => {
            assert_eq!(
                message,
                ServerMessage::Reload,
                "Expected Reload message after image modification"
            );
        }
        Err(_) => {
            panic!("Timeout waiting for reload after image modification");
        }
    }
}

#[tokio::test]
async fn test_new_markdown_file_triggers_file_added() {
    let (server, temp_dir) = create_directory_server_with_http().await;
    let mut websocket = server.get_websocket("/ws").await.into_websocket().await;

    // Wait a moment for initial setup
    tokio::time::sleep(Duration::from_millis(300)).await;

    // Create a new markdown file
    let new_file_path = temp_dir.path().join("new-file.md");
    fs::write(&new_file_path, "# New File").expect("Failed to create new file");

    // Should receive FileAdded message via WebSocket
    let update_result = tokio::time::timeout(
        Duration::from_secs(WEBSOCKET_TIMEOUT_SECS),
        websocket.receive_json::<ServerMessage>(),
    )
    .await;

    match update_result {
        Ok(message) => match message {
            ServerMessage::FileAdded { name } => {
                assert_eq!(name, "new-file.md", "Expected new-file.md to be added");
            }
            _ => panic!("Expected FileAdded message, got {:?}", message),
        },
        Err(_) => {
            panic!("Timeout waiting for FileAdded after creating new markdown file");
        }
    }
}

#[tokio::test]
async fn test_markdown_file_removal_triggers_file_removed() {
    let (server, temp_dir) = create_directory_server_with_http().await;
    let mut websocket = server.get_websocket("/ws").await.into_websocket().await;

    // Wait a moment for initial setup
    tokio::time::sleep(Duration::from_millis(300)).await;

    // Remove one of the existing test files
    let file_to_remove = temp_dir.path().join("test1.md");
    fs::remove_file(&file_to_remove).expect("Failed to remove file");

    // Should receive FileRemoved message via WebSocket (after delayed rescan)
    let update_result = tokio::time::timeout(
        Duration::from_secs(WEBSOCKET_TIMEOUT_SECS),
        websocket.receive_json::<ServerMessage>(),
    )
    .await;

    match update_result {
        Ok(message) => match message {
            ServerMessage::FileRemoved { name } => {
                assert_eq!(name, "test1.md", "Expected test1.md to be removed");
            }
            _ => panic!("Expected FileRemoved message, got {:?}", message),
        },
        Err(_) => {
            panic!("Timeout waiting for FileRemoved after deleting markdown file");
        }
    }
}

#[tokio::test]
async fn test_file_rename_triggers_file_renamed() {
    let (server, temp_dir) = create_directory_server_with_http().await;
    let mut websocket = server.get_websocket("/ws").await.into_websocket().await;

    // Wait a moment for initial setup
    tokio::time::sleep(Duration::from_millis(300)).await;

    // Rename one of the existing test files
    let old_path = temp_dir.path().join("test1.md");
    let new_path = temp_dir.path().join("renamed-test.md");
    fs::rename(&old_path, &new_path).expect("Failed to rename file");

    // Should receive FileRenamed message via WebSocket
    let update_result = tokio::time::timeout(
        Duration::from_secs(WEBSOCKET_TIMEOUT_SECS),
        websocket.receive_json::<ServerMessage>(),
    )
    .await;

    match update_result {
        Ok(message) => match message {
            ServerMessage::FileRenamed { old_name, new_name } => {
                assert_eq!(old_name, "test1.md", "Expected old name to be test1.md");
                assert_eq!(
                    new_name, "renamed-test.md",
                    "Expected new name to be renamed-test.md"
                );
            }
            _ => panic!("Expected FileRenamed message, got {:?}", message),
        },
        Err(_) => {
            panic!("Timeout waiting for FileRenamed after renaming file");
        }
    }
}
