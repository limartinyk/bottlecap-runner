// Prevents additional console window on Windows in release
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tauri::{Manager, State};
use tokio::sync::Mutex;
use tokio_tungstenite::{connect_async, tungstenite::Message};

// Connection state shared across the app
struct AppState {
    connection: Arc<Mutex<Option<ConnectionHandle>>>,
}

struct ConnectionHandle {
    cancel_token: tokio::sync::oneshot::Sender<()>,
}

// Message types for WebSocket communication
#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "type")]
enum ServerMessage {
    #[serde(rename = "auth_success")]
    AuthSuccess { runnerId: String },
    #[serde(rename = "chat_request")]
    ChatRequest {
        requestId: String,
        model: String,
        messages: Vec<ChatMessage>,
        options: ChatOptions,
    },
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "type")]
enum ClientMessage {
    #[serde(rename = "auth")]
    Auth { token: String },
    #[serde(rename = "chat_response")]
    ChatResponse {
        requestId: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        content: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        chunk: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        done: Option<bool>,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        usage: Option<Usage>,
    },
    #[serde(rename = "status")]
    Status {
        status: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        models: Option<Vec<String>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        deviceName: Option<String>,
    },
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct ChatMessage {
    role: String,
    content: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
struct ChatOptions {
    temperature: Option<f32>,
    max_tokens: Option<i32>,
    stream: Option<bool>,
}

#[derive(Serialize, Deserialize, Debug)]
struct Usage {
    inputTokens: i32,
    outputTokens: i32,
}

#[derive(Serialize, Deserialize, Debug)]
struct OllamaResponse {
    message: Option<OllamaMessage>,
    done: Option<bool>,
    prompt_eval_count: Option<i32>,
    eval_count: Option<i32>,
}

#[derive(Serialize, Deserialize, Debug)]
struct OllamaMessage {
    role: String,
    content: String,
}

#[derive(Serialize, Deserialize, Debug)]
struct OllamaModelsResponse {
    models: Vec<OllamaModel>,
}

#[derive(Serialize, Deserialize, Debug)]
struct OllamaModel {
    name: String,
}

// Tauri commands
#[tauri::command]
async fn get_saved_token() -> Result<Option<String>, String> {
    let entry = keyring::Entry::new("bottlecap-runner", "token")
        .map_err(|e| e.to_string())?;

    match entry.get_password() {
        Ok(token) => Ok(Some(token)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

#[tauri::command]
async fn save_token(token: String) -> Result<(), String> {
    let entry = keyring::Entry::new("bottlecap-runner", "token")
        .map_err(|e| e.to_string())?;
    entry.set_password(&token).map_err(|e| e.to_string())
}

#[tauri::command]
async fn clear_token() -> Result<(), String> {
    let entry = keyring::Entry::new("bottlecap-runner", "token")
        .map_err(|e| e.to_string())?;
    match entry.delete_password() {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}

#[tauri::command]
async fn check_ollama() -> Result<bool, String> {
    let client = reqwest::Client::new();
    match client.get("http://localhost:11434/api/tags").send().await {
        Ok(resp) => Ok(resp.status().is_success()),
        Err(_) => Ok(false),
    }
}

async fn get_ollama_models() -> Result<Vec<String>, String> {
    let client = reqwest::Client::new();
    let response = client
        .get("http://localhost:11434/api/tags")
        .send()
        .await
        .map_err(|e| e.to_string())?;

    let data: OllamaModelsResponse = response.json().await.map_err(|e| e.to_string())?;
    Ok(data.models.into_iter().map(|m| m.name).collect())
}

async fn forward_to_ollama(
    model: &str,
    messages: &[ChatMessage],
    options: &ChatOptions,
) -> Result<(String, Usage), String> {
    let client = reqwest::Client::new();

    let body = serde_json::json!({
        "model": model,
        "messages": messages,
        "stream": false,
        "options": {
            "temperature": options.temperature,
            "num_predict": options.max_tokens,
        }
    });

    let response = client
        .post("http://localhost:11434/api/chat")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Ollama request failed: {}", e))?;

    if !response.status().is_success() {
        return Err(format!("Ollama error: {}", response.status()));
    }

    let data: OllamaResponse = response.json().await.map_err(|e| e.to_string())?;

    let content = data
        .message
        .map(|m| m.content)
        .unwrap_or_default();

    let usage = Usage {
        inputTokens: data.prompt_eval_count.unwrap_or(0),
        outputTokens: data.eval_count.unwrap_or(0),
    };

    Ok((content, usage))
}

#[tauri::command]
async fn connect_to_partykit(
    token: String,
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    // Disconnect existing connection if any
    {
        let mut conn = state.connection.lock().await;
        if let Some(handle) = conn.take() {
            let _ = handle.cancel_token.send(());
        }
    }

    // Partykit WebSocket URL
    let ws_url = "wss://bottlecap-runners.limartinyk.partykit.dev/party/main".to_string();

    // Create cancel token
    let (cancel_tx, mut cancel_rx) = tokio::sync::oneshot::channel::<()>();

    // Store connection handle
    {
        let mut conn = state.connection.lock().await;
        *conn = Some(ConnectionHandle {
            cancel_token: cancel_tx,
        });
    }

    // Spawn WebSocket connection task
    let app_handle_clone = app_handle.clone();
    tokio::spawn(async move {
        // Emit connecting status
        let _ = app_handle_clone.emit_all("connection-status", serde_json::json!({
            "status": "connecting"
        }));

        // Connect to WebSocket
        let ws_result = connect_async(&ws_url).await;

        let (ws_stream, _) = match ws_result {
            Ok(stream) => stream,
            Err(e) => {
                let _ = app_handle_clone.emit_all("connection-status", serde_json::json!({
                    "status": "error",
                    "error": format!("WebSocket connection failed: {}", e)
                }));
                return;
            }
        };

        let (mut write, mut read) = ws_stream.split();

        // Send auth message
        let auth_msg = ClientMessage::Auth { token };
        if let Ok(json) = serde_json::to_string(&auth_msg) {
            if let Err(e) = write.send(Message::Text(json)).await {
                let _ = app_handle_clone.emit_all("connection-status", serde_json::json!({
                    "status": "error",
                    "error": format!("Failed to send auth: {}", e)
                }));
                return;
            }
        }

        // Process messages
        loop {
            tokio::select! {
                _ = &mut cancel_rx => {
                    let _ = app_handle_clone.emit_all("connection-status", serde_json::json!({
                        "status": "disconnected"
                    }));
                    break;
                }
                msg = read.next() => {
                    match msg {
                        Some(Ok(Message::Text(text))) => {
                            if let Ok(server_msg) = serde_json::from_str::<ServerMessage>(&text) {
                                match server_msg {
                                    ServerMessage::AuthSuccess { runnerId: _ } => {
                                        let _ = app_handle_clone.emit_all("connection-status", serde_json::json!({
                                            "status": "connected"
                                        }));

                                        // Get and send available models
                                        if let Ok(models) = get_ollama_models().await {
                                            let _ = app_handle_clone.emit_all("models-updated", &models);

                                            // Send status to server
                                            let hostname = hostname::get()
                                                .ok()
                                                .and_then(|h| h.into_string().ok());

                                            let status_msg = ClientMessage::Status {
                                                status: "online".to_string(),
                                                models: Some(models),
                                                deviceName: hostname,
                                            };

                                            if let Ok(json) = serde_json::to_string(&status_msg) {
                                                let _ = write.send(Message::Text(json)).await;
                                            }
                                        }
                                    }
                                    ServerMessage::ChatRequest { requestId, model, messages, options } => {
                                        let _ = app_handle_clone.emit_all("log-message", serde_json::json!({
                                            "message": format!("Request for model: {}", model),
                                            "type": "info"
                                        }));

                                        // Forward to Ollama
                                        let response = match forward_to_ollama(&model, &messages, &options).await {
                                            Ok((content, usage)) => {
                                                let _ = app_handle_clone.emit_all("log-message", serde_json::json!({
                                                    "message": format!("Completed: {} tokens", usage.inputTokens + usage.outputTokens),
                                                    "type": "success"
                                                }));

                                                ClientMessage::ChatResponse {
                                                    requestId,
                                                    content: Some(content),
                                                    chunk: None,
                                                    done: Some(true),
                                                    error: None,
                                                    usage: Some(usage),
                                                }
                                            }
                                            Err(e) => {
                                                let _ = app_handle_clone.emit_all("log-message", serde_json::json!({
                                                    "message": format!("Error: {}", e),
                                                    "type": "error"
                                                }));

                                                ClientMessage::ChatResponse {
                                                    requestId,
                                                    content: None,
                                                    chunk: None,
                                                    done: Some(true),
                                                    error: Some(e),
                                                    usage: None,
                                                }
                                            }
                                        };

                                        if let Ok(json) = serde_json::to_string(&response) {
                                            let _ = write.send(Message::Text(json)).await;
                                        }
                                    }
                                }
                            }
                        }
                        Some(Ok(Message::Ping(data))) => {
                            let _ = write.send(Message::Pong(data)).await;
                        }
                        Some(Ok(Message::Close(_))) | None => {
                            let _ = app_handle_clone.emit_all("connection-status", serde_json::json!({
                                "status": "disconnected"
                            }));
                            break;
                        }
                        Some(Err(e)) => {
                            let _ = app_handle_clone.emit_all("connection-status", serde_json::json!({
                                "status": "error",
                                "error": format!("WebSocket error: {}", e)
                            }));
                            break;
                        }
                        _ => {}
                    }
                }
            }
        }
    });

    Ok(())
}

#[tauri::command]
async fn disconnect(state: State<'_, AppState>) -> Result<(), String> {
    let mut conn = state.connection.lock().await;
    if let Some(handle) = conn.take() {
        let _ = handle.cancel_token.send(());
    }
    Ok(())
}

fn main() {
    tauri::Builder::default()
        .manage(AppState {
            connection: Arc::new(Mutex::new(None)),
        })
        .invoke_handler(tauri::generate_handler![
            get_saved_token,
            save_token,
            clear_token,
            check_ollama,
            connect_to_partykit,
            disconnect,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
