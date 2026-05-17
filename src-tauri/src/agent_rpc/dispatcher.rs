//! Dispatches `invoke` WebSocket messages to the command registry and
//! streams `invoke_progress` / `invoke_result` responses back.

use crate::agent_rpc::registry::{Ctx, Progress, REGISTRY};
use serde_json::{json, Value};
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use tokio_tungstenite::tungstenite::Message;

type WsWriter = Arc<Mutex<
    futures::stream::SplitSink<
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
        Message,
    >,
>>;

/// Entry point called from `websocket.rs` for `type = "invoke"` messages.
pub async fn dispatch(
    message: &Value,
    write: WsWriter,
    app: Option<tauri::AppHandle<tauri::Wry>>,
) {
    let request_id = match message.get("request_id").and_then(|v| v.as_str()) {
        Some(id) => id.to_string(),
        None => {
            eprintln!("[rpc] invoke missing request_id — ignored");
            return;
        }
    };
    let command_name = match message.get("command").and_then(|v| v.as_str()) {
        Some(n) => n.to_string(),
        None => {
            send_error(&write, &request_id, "missing field: command").await;
            return;
        }
    };
    let args = message.get("args").cloned().unwrap_or(json!({}));

    let cmd = match REGISTRY.get(&command_name) {
        Some(c) => c,
        None => {
            send_error(&write, &request_id, &format!("unknown command: {command_name}")).await;
            return;
        }
    };

    let (progress_tx, mut progress_rx) = mpsc::channel::<Progress>(64);

    // Stream progress events back while the command runs.
    let write_for_progress = write.clone();
    let req_id_for_progress = request_id.clone();
    tokio::spawn(async move {
        while let Some(p) = progress_rx.recv().await {
            send_ws(
                &write_for_progress,
                json!({
                    "type":       "invoke_progress",
                    "request_id": req_id_for_progress,
                    "data":       p.data,
                }),
            )
            .await;
        }
    });

    let ctx = Ctx { args, progress: progress_tx, app };

    match cmd.execute(ctx).await {
        Ok(data) => {
            send_ws(
                &write,
                json!({
                    "type":       "invoke_result",
                    "request_id": request_id,
                    "ok":         true,
                    "data":       data,
                }),
            )
            .await;
        }
        Err(e) => {
            send_error(&write, &request_id, &e).await;
        }
    }
}

async fn send_error(write: &WsWriter, request_id: &str, error: &str) {
    send_ws(
        write,
        json!({
            "type":       "invoke_result",
            "request_id": request_id,
            "ok":         false,
            "error":      error,
        }),
    )
    .await;
}

async fn send_ws(write: &WsWriter, msg: Value) {
    use futures::SinkExt;
    let mut guard = write.lock().await;
    let _ = guard.send(Message::Text(msg.to_string())).await;
}
