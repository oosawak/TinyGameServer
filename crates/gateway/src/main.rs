use std::{env, sync::Arc};

use anyhow::Result;
use axum::{
    Json, Router,
    extract::{Path, State, WebSocketUpgrade, ws::{Message, WebSocket}},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, put},
};
use futures_util::{SinkExt, StreamExt};
use gameforge_common::{EnsureGroupResponse, HealthResponse, ValuePayload};
use reqwest::Client;
use tokio_tungstenite::{connect_async, tungstenite};
use tower_http::{cors::CorsLayer, trace::TraceLayer};
use tracing::{info, warn};

#[derive(Clone)]
struct AppState {
    supervisor_url: Arc<String>,
    http: Client,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let bind = env::var("GAMEFORGE_GATEWAY_BIND").unwrap_or_else(|_| "0.0.0.0:8787".into());
    let supervisor_url = env::var("GAMEFORGE_SUPERVISOR_URL")
        .unwrap_or_else(|_| "http://127.0.0.1:8788".into());

    let state = AppState {
        supervisor_url: Arc::new(supervisor_url),
        http: Client::new(),
    };

    let app = Router::new()
        .route("/health", get(health))
        .route("/api/groups/{group_id}/state/{key}", get(get_state).put(put_state))
        .route("/ws/{group_id}", get(ws_handler))
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(&bind).await?;
    info!(%bind, "gateway started");
    axum::serve(listener, app).await?;
    Ok(())
}

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse { ok: true, service: "gateway".into() })
}

async fn ensure_group(state: &AppState, group_id: &str) -> Result<EnsureGroupResponse, (StatusCode, String)> {
    let url = format!("{}/groups/{}/ensure", state.supervisor_url, group_id);
    let response = state.http.post(url).send().await.map_err(bad_gateway)?;
    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err((StatusCode::BAD_GATEWAY, format!("supervisor {status}: {body}")));
    }
    response.json().await.map_err(bad_gateway)
}

async fn get_state(
    Path((group_id, key)): Path<(String, String)>,
    State(state): State<AppState>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let group = ensure_group(&state, &group_id).await?;
    let url = format!("http://127.0.0.1:{}/api/state/{}", group.port, key);
    let response = state.http.get(url).send().await.map_err(bad_gateway)?;
    relay_json(response).await
}

async fn put_state(
    Path((group_id, key)): Path<(String, String)>,
    State(state): State<AppState>,
    Json(payload): Json<ValuePayload>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let group = ensure_group(&state, &group_id).await?;
    let url = format!("http://127.0.0.1:{}/api/state/{}", group.port, key);
    let response = state.http.put(url).json(&payload).send().await.map_err(bad_gateway)?;
    relay_json(response).await
}

async fn relay_json(response: reqwest::Response) -> Result<impl IntoResponse, (StatusCode, String)> {
    let upstream = response.status();
    let body = response.text().await.map_err(bad_gateway)?;
    let status = StatusCode::from_u16(upstream.as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    Ok((status, [(axum::http::header::CONTENT_TYPE, "application/json")], body))
}

async fn ws_handler(
    Path(group_id): Path<String>,
    State(state): State<AppState>,
    ws: WebSocketUpgrade,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let group = ensure_group(&state, &group_id).await?;
    Ok(ws.on_upgrade(move |socket| proxy_ws(socket, group.port)))
}

async fn proxy_ws(client_socket: WebSocket, port: u16) {
    let url = format!("ws://127.0.0.1:{port}/ws");
    let Ok((upstream, _)) = connect_async(&url).await else {
        warn!(%url, "failed to connect upstream websocket");
        return;
    };

    let (mut client_tx, mut client_rx) = client_socket.split();
    let (mut upstream_tx, mut upstream_rx) = upstream.split();

    let client_to_upstream = async {
        while let Some(Ok(msg)) = client_rx.next().await {
            let mapped = match msg {
                Message::Text(v) => tungstenite::Message::Text(v.to_string().into()),
                Message::Binary(v) => tungstenite::Message::Binary(v.to_vec().into()),
                Message::Ping(v) => tungstenite::Message::Ping(v.to_vec().into()),
                Message::Pong(v) => tungstenite::Message::Pong(v.to_vec().into()),
                Message::Close(_) => tungstenite::Message::Close(None),
            };
            if upstream_tx.send(mapped).await.is_err() { break; }
        }
    };

    let upstream_to_client = async {
        while let Some(Ok(msg)) = upstream_rx.next().await {
            let mapped = match msg {
                tungstenite::Message::Text(v) => Message::Text(v.to_string().into()),
                tungstenite::Message::Binary(v) => Message::Binary(v.to_vec().into()),
                tungstenite::Message::Ping(v) => Message::Ping(v.to_vec().into()),
                tungstenite::Message::Pong(v) => Message::Pong(v.to_vec().into()),
                tungstenite::Message::Close(_) => Message::Close(None),
                tungstenite::Message::Frame(_) => continue,
            };
            if client_tx.send(mapped).await.is_err() { break; }
        }
    };

    tokio::select! {
        _ = client_to_upstream => {},
        _ = upstream_to_client => {},
    }
}

fn bad_gateway<E: std::fmt::Display>(err: E) -> (StatusCode, String) {
    (StatusCode::BAD_GATEWAY, err.to_string())
}
