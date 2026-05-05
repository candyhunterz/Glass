use axum::{
    extract::{Path, State},
    http::{header, StatusCode},
    response::{IntoResponse, Json, Response},
    routing::get,
    Router,
};
use rust_embed::Embed;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Embed)]
#[folder = "tools/run-analyzer/"]
struct DashboardAssets;

struct AppState {
    glass_dir: PathBuf,
}

/// Serve the embedded `index.html` at `/`.
async fn index() -> impl IntoResponse {
    serve_embedded("index.html")
}

/// Serve embedded static assets at their original path.
async fn static_asset(Path(path): Path<String>) -> impl IntoResponse {
    serve_embedded(&path)
}

/// Look up a file in the embedded assets and return it with the correct MIME type.
fn serve_embedded(path: &str) -> Response {
    let asset_path = format!("dist/{path}");
    match DashboardAssets::get(&asset_path) {
        Some(file) => {
            let mime = mime_guess::from_path(path).first_or_octet_stream();
            (
                StatusCode::OK,
                [(header::CONTENT_TYPE, mime.as_ref().to_string())],
                file.data.to_vec(),
            )
                .into_response()
        }
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

/// `GET /api/files` — JSON array of filenames in the `.glass/` directory.
async fn list_files(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    match std::fs::read_dir(&state.glass_dir) {
        Ok(entries) => {
            let mut names: Vec<String> = entries
                .filter_map(|e| e.ok())
                .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
                .filter_map(|e| e.file_name().into_string().ok())
                .collect();
            names.sort();
            Json(names).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to read directory: {e}"),
        )
            .into_response(),
    }
}

/// `GET /api/files/:name` — raw content of a single file.
async fn read_file(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> impl IntoResponse {
    // Prevent path traversal
    if name.contains('/') || name.contains('\\') || name.contains("..") {
        return StatusCode::BAD_REQUEST.into_response();
    }

    let path = state.glass_dir.join(&name);
    match std::fs::read_to_string(&path) {
        Ok(content) => (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
            content,
        )
            .into_response(),
        Err(_) => StatusCode::NOT_FOUND.into_response(),
    }
}

/// `GET /api/dir` — JSON with the `.glass/` directory path.
async fn get_dir(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    Json(serde_json::json!({ "path": state.glass_dir.display().to_string() }))
}

/// Run the analyze HTTP server.
pub async fn run(dir: Option<String>, port: u16, no_open: bool) -> anyhow::Result<()> {
    let glass_dir = match dir {
        Some(d) => PathBuf::from(d),
        None => std::env::current_dir()?.join(".glass"),
    };

    if !glass_dir.is_dir() {
        anyhow::bail!(
            "Directory not found: {}\nRun from a project with a .glass/ directory, or use --dir <path>",
            glass_dir.display()
        );
    }

    let state = Arc::new(AppState { glass_dir });

    let app = Router::new()
        .route("/", get(index))
        .route("/api/files", get(list_files))
        .route("/api/files/{name}", get(read_file))
        .route("/api/dir", get(get_dir))
        .route("/assets/{*path}", get(static_asset))
        .with_state(state);

    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let listener = tokio::net::TcpListener::bind(addr).await?;
    let url = format!("http://localhost:{port}");
    println!("Glass Run Analyzer serving at {url}");

    if !no_open {
        open_browser(&url);
    }

    println!("Press Ctrl+C to stop");
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    Ok(())
}

/// Wait for Ctrl+C.
async fn shutdown_signal() {
    tokio::signal::ctrl_c()
        .await
        .expect("failed to install Ctrl+C handler");
    println!("\nShutting down...");
}

/// Open a URL in the default browser.
fn open_browser(url: &str) {
    #[cfg(target_os = "windows")]
    {
        let _ = std::process::Command::new("cmd")
            .args(["/C", "start", url])
            .spawn();
    }
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(url).spawn();
    }
    #[cfg(target_os = "linux")]
    {
        let _ = std::process::Command::new("xdg-open").arg(url).spawn();
    }
}
