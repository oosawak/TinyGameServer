use std::{collections::HashMap, env, path::PathBuf, sync::Arc};

use anyhow::{Context, Result};
use axum::{Json, Router, extract::{Path, State}, routing::{get, post}};
use gameforge_common::{EnsureGroupResponse, HealthResponse};
use tokio::{process::{Child, Command}, sync::Mutex};
use tower_http::trace::TraceLayer;
use tracing::{info, warn};

struct GroupHandle {
    child: Child,
    port: u16,
}

#[derive(Clone)]
struct AppState {
    groups: Arc<Mutex<HashMap<String, GroupHandle>>>,
    data_root: PathBuf,
    group_binary: PathBuf,
    idle_secs: u64,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let bind = env::var("GAMEFORGE_SUPERVISOR_BIND").unwrap_or_else(|_| "127.0.0.1:8788".into());
    let data_root = PathBuf::from(env::var("GAMEFORGE_DATA_ROOT").unwrap_or_else(|_| "./data/groups".into()));
    let idle_secs = env::var("GAMEFORGE_GROUP_IDLE_SECONDS")
        .ok().and_then(|v| v.parse().ok()).unwrap_or(300);
    let group_binary = env::var("GAMEFORGE_GROUP_SERVER")
        .map(PathBuf::from)
        .unwrap_or_else(|_| default_group_binary());

    tokio::fs::create_dir_all(&data_root).await?;

    let state = AppState {
        groups: Arc::new(Mutex::new(HashMap::new())),
        data_root,
        group_binary,
        idle_secs,
    };

    let app = Router::new()
        .route("/health", get(health))
        .route("/groups/{group_id}/ensure", post(ensure_group))
        .route("/groups", get(list_groups))
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(&bind).await?;
    info!(%bind, "supervisor started");
    axum::serve(listener, app).await?;
    Ok(())
}

fn default_group_binary() -> PathBuf {
    let file = if cfg!(windows) { "gameforge-group-server.exe" } else { "gameforge-group-server" };
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join(file)))
        .unwrap_or_else(|| PathBuf::from(file))
}

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse { ok: true, service: "supervisor".into() })
}

async fn list_groups(State(state): State<AppState>) -> Json<Vec<EnsureGroupResponse>> {
    let mut groups = state.groups.lock().await;
    let mut dead = Vec::new();
    let mut result = Vec::new();

    for (group_id, handle) in groups.iter_mut() {
        match handle.child.try_wait() {
            Ok(None) => result.push(EnsureGroupResponse {
                group_id: group_id.clone(),
                port: handle.port,
                pid: handle.child.id(),
            }),
            Ok(Some(_)) | Err(_) => dead.push(group_id.clone()),
        }
    }
    for id in dead { groups.remove(&id); }
    Json(result)
}

async fn ensure_group(
    Path(group_id): Path<String>,
    State(state): State<AppState>,
) -> Result<Json<EnsureGroupResponse>, (axum::http::StatusCode, String)> {
    validate_group_id(&group_id)?;

    let mut groups = state.groups.lock().await;
    if let Some(handle) = groups.get_mut(&group_id) {
        match handle.child.try_wait() {
            Ok(None) => {
                return Ok(Json(EnsureGroupResponse {
                    group_id,
                    port: handle.port,
                    pid: handle.child.id(),
                }));
            }
            Ok(Some(status)) => {
                info!(%group_id, ?status, "old group process exited; respawning");
            }
            Err(err) => warn!(%group_id, error=%err, "could not inspect group process; respawning"),
        }
        groups.remove(&group_id);
    }

    let port = allocate_local_port().await.map_err(internal_error)?;
    let shard = &group_id[..group_id.len().min(2)];
    let db_path = state.data_root.join(shard).join(&group_id).join("game.db");
    if let Some(parent) = db_path.parent() {
        tokio::fs::create_dir_all(parent).await.map_err(internal_error)?;
    }

    let child = Command::new(&state.group_binary)
        .arg("--group").arg(&group_id)
        .arg("--db").arg(&db_path)
        .arg("--port").arg(port.to_string())
        .arg("--idle-seconds").arg(state.idle_secs.to_string())
        .kill_on_drop(false)
        .spawn()
        .with_context(|| format!("failed to spawn {}", state.group_binary.display()))
        .map_err(internal_error)?;

    let pid = child.id();
    groups.insert(group_id.clone(), GroupHandle { child, port });
    info!(%group_id, %port, ?pid, "group process spawned");

    drop(groups);
    wait_until_ready(port).await.map_err(internal_error)?;

    Ok(Json(EnsureGroupResponse { group_id, port, pid }))
}

fn validate_group_id(group_id: &str) -> Result<(), (axum::http::StatusCode, String)> {
    let valid = !group_id.is_empty()
        && group_id.len() <= 64
        && group_id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if valid { Ok(()) } else {
        Err((axum::http::StatusCode::BAD_REQUEST, "invalid group id".into()))
    }
}

async fn allocate_local_port() -> Result<u16> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    Ok(listener.local_addr()?.port())
}

async fn wait_until_ready(port: u16) -> Result<()> {
    let addr = format!("127.0.0.1:{port}");
    for _ in 0..50 {
        if tokio::net::TcpStream::connect(&addr).await.is_ok() {
            return Ok(());
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    anyhow::bail!("group server did not become ready on {addr}")
}

fn internal_error<E: std::fmt::Display>(err: E) -> (axum::http::StatusCode, String) {
    (axum::http::StatusCode::INTERNAL_SERVER_ERROR, err.to_string())
}
