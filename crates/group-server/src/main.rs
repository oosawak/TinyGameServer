use std::{
    env,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicU64, AtomicUsize, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result};
use axum::{
    Json, Router,
    extract::{Path, State, WebSocketUpgrade, ws::{Message, WebSocket}},
    response::IntoResponse,
    routing::{get, put},
};
use futures_util::{SinkExt, StreamExt};
use gameforge_common::{HealthResponse, ValuePayload};
use sqlx::{SqlitePool, sqlite::SqlitePoolOptions};
use tokio::sync::broadcast;
use tower_http::trace::TraceLayer;
use tracing::{info, warn};

#[derive(Clone)]
struct AppState {
    group_id: String,
    db: SqlitePool,
    tx: broadcast::Sender<Vec<u8>>,
    last_activity: Arc<AtomicU64>,
    active_ws: Arc<AtomicUsize>,
}

impl AppState {
    fn touch(&self) {
        self.last_activity.store(now_secs(), Ordering::Relaxed);
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let args: Vec<String> = env::args().collect();
    let group_id = arg_value(&args, "--group").context("missing --group")?;
    let db_path = PathBuf::from(arg_value(&args, "--db").context("missing --db")?);
    let port: u16 = arg_value(&args, "--port")
        .context("missing --port")?
        .parse()
        .context("invalid --port")?;
    let idle_secs: u64 = arg_value(&args, "--idle-seconds")
        .unwrap_or_else(|| "300".into())
        .parse()
        .context("invalid --idle-seconds")?;

    if let Some(parent) = db_path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    let db_url = format!("sqlite://{}?mode=rwc", db_path.to_string_lossy().replace('\\', "/"));
    let db = SqlitePoolOptions::new()
        .max_connections(5)
        .connect(&db_url)
        .await
        .with_context(|| format!("failed to open SQLite at {}", db_path.display()))?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS kv (key TEXT PRIMARY KEY, value TEXT NOT NULL, updated_at INTEGER NOT NULL)"
    )
    .execute(&db)
    .await?;

    sqlx::query("PRAGMA journal_mode=WAL").execute(&db).await?;
    sqlx::query("PRAGMA synchronous=NORMAL").execute(&db).await?;

    let (tx, _) = broadcast::channel(256);
    let state = AppState {
        group_id: group_id.clone(),
        db,
        tx,
        last_activity: Arc::new(AtomicU64::new(now_secs())),
        active_ws: Arc::new(AtomicUsize::new(0)),
    };

    spawn_idle_shutdown(state.clone(), idle_secs);

    let app = Router::new()
        .route("/health", get(health))
        .route("/api/state/{key}", get(get_value).put(put_value))
        .route("/ws", get(ws_handler))
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let addr = format!("127.0.0.1:{port}");
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    info!(%group_id, %addr, "group server started");
    axum::serve(listener, app).await?;
    Ok(())
}

fn arg_value(args: &[String], key: &str) -> Option<String> {
    args.windows(2)
        .find(|pair| pair[0] == key)
        .map(|pair| pair[1].clone())
}

async fn health(State(state): State<AppState>) -> impl IntoResponse {
    Json(HealthResponse {
        ok: true,
        service: format!("group:{}", state.group_id),
    })
}

async fn get_value(
    Path(key): Path<String>,
    State(state): State<AppState>,
) -> Result<Json<ValuePayload>, (axum::http::StatusCode, String)> {
    state.touch();
    let row: Option<(String,)> = sqlx::query_as("SELECT value FROM kv WHERE key = ?")
        .bind(&key)
        .fetch_optional(&state.db)
        .await
        .map_err(internal_error)?;

    match row {
        Some((value,)) => Ok(Json(ValuePayload { value })),
        None => Err((axum::http::StatusCode::NOT_FOUND, "key not found".into())),
    }
}

async fn put_value(
    Path(key): Path<String>,
    State(state): State<AppState>,
    Json(payload): Json<ValuePayload>,
) -> Result<Json<ValuePayload>, (axum::http::StatusCode, String)> {
    state.touch();
    sqlx::query(
        "INSERT INTO kv(key, value, updated_at) VALUES(?, ?, ?) \
         ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at"
    )
    .bind(&key)
    .bind(&payload.value)
    .bind(now_secs() as i64)
    .execute(&state.db)
    .await
    .map_err(internal_error)?;

    Ok(Json(payload))
}

async fn ws_handler(ws: WebSocketUpgrade, State(state): State<AppState>) -> impl IntoResponse {
    state.touch();
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

async fn handle_socket(socket: WebSocket, state: AppState) {
    state.active_ws.fetch_add(1, Ordering::Relaxed);
    state.touch();

    let (mut ws_tx, mut ws_rx) = socket.split();
    let mut rx = state.tx.subscribe();

    let outgoing = tokio::spawn(async move {
        while let Ok(payload) = rx.recv().await {
            if ws_tx.send(Message::Binary(payload.into())).await.is_err() {
                break;
            }
        }
    });

    while let Some(Ok(message)) = ws_rx.next().await {
        state.touch();
        match message {
            Message::Text(text) => {
                let _ = state.tx.send(text.as_bytes().to_vec());
            }
            Message::Binary(bytes) => {
                let _ = state.tx.send(bytes.to_vec());
            }
            Message::Close(_) => break,
            Message::Ping(_) | Message::Pong(_) => {}
        }
    }

    outgoing.abort();
    state.active_ws.fetch_sub(1, Ordering::Relaxed);
    state.touch();
}

fn spawn_idle_shutdown(state: AppState, idle_secs: u64) {
    tokio::spawn(async move {
        let interval = Duration::from_secs(5);
        loop {
            tokio::time::sleep(interval).await;
            if state.active_ws.load(Ordering::Relaxed) > 0 {
                continue;
            }
            let last = state.last_activity.load(Ordering::Relaxed);
            if now_secs().saturating_sub(last) >= idle_secs {
                info!(group_id = %state.group_id, idle_secs, "idle timeout reached; exiting");
                state.db.close().await;
                std::process::exit(0);
            }
        }
    });
}

fn internal_error(err: sqlx::Error) -> (axum::http::StatusCode, String) {
    warn!(error = %err, "database error");
    (axum::http::StatusCode::INTERNAL_SERVER_ERROR, err.to_string())
}
