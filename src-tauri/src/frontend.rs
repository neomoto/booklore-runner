//! Frontend HTTP Server - serves Angular frontend and proxies API calls
//! Replicates nginx functionality from Docker setup

use axum::{
    body::Body,
    extract::{
        Request, State,
        ws::{WebSocket, WebSocketUpgrade, Message},
    },
    http::{StatusCode, header, Method},
    response::{IntoResponse, Response},
    routing::get,
    Router,
};
use futures_util::{SinkExt, StreamExt};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::Mutex;
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message as TungsteniteMessage};
use tower_http::cors::{Any, CorsLayer};
use tower_http::services::ServeDir;
use tracing::{error, info, debug, warn};

/// Frontend server state
#[derive(Clone)]
pub struct FrontendServerState {
    pub backend_port: u16,
    pub frontend_dir: PathBuf,
}

/// Frontend server handle
static SERVER_HANDLE: tokio::sync::OnceCell<Mutex<Option<tokio::task::JoinHandle<()>>>> = 
    tokio::sync::OnceCell::const_new();

async fn get_handle() -> &'static Mutex<Option<tokio::task::JoinHandle<()>>> {
    SERVER_HANDLE.get_or_init(|| async { Mutex::new(None) }).await
}

/// Start the frontend HTTP server
/// Serves Angular frontend on specified port and proxies /api to backend
pub async fn start(frontend_port: u16, backend_port: u16, frontend_dir: PathBuf) -> Result<(), String> {
    info!("Starting frontend server on port {}...", frontend_port);
    info!("  Frontend directory: {:?}", frontend_dir);
    info!("  Backend port for proxy: {}", backend_port);
    
    if !frontend_dir.exists() {
        return Err(format!("Frontend directory does not exist: {:?}", frontend_dir));
    }
    
    let index_html = frontend_dir.join("index.html");
    if !index_html.exists() {
        return Err(format!("index.html not found in frontend directory: {:?}", index_html));
    }
    
    let state = Arc::new(FrontendServerState {
        backend_port,
        frontend_dir: frontend_dir.clone(),
    });
    
    // Create static file service
    // We do NOT set specific fallback here because we want to use our custom serve_index handler
    // for SPA routing, so we can inject the CSS.
    // But ServeDir returns 404 for missing files, it doesn't fall through to axum fallback automatically
    // unless configured.
    // The solution is to use fallback_service on ServeDir itself.
    let serve_dir = ServeDir::new(&frontend_dir)
        .not_found_service(get(serve_index).with_state(state.clone()));
    
    // Configure CORS to allow requests from the same origin
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([Method::GET, Method::POST, Method::PUT, Method::DELETE, Method::PATCH, Method::OPTIONS])
        .allow_headers([header::CONTENT_TYPE, header::AUTHORIZATION, header::ACCEPT]);
    
    // Build the router with API proxy and static file serving
    let app = Router::new()
        // API proxy routes - all HTTP methods
        .route("/api/{*rest}", get(proxy_handler).post(proxy_handler).put(proxy_handler).delete(proxy_handler).patch(proxy_handler))
        // Actuator endpoint proxy
        .route("/actuator/{*rest}", get(proxy_handler))
        // WebSocket proxy endpoint
        .route("/ws", get(ws_proxy_handler))
        // Explicit index routes to ensure injection works for root
        .route("/", get(serve_index))
        .route("/index.html", get(serve_index))
        .with_state(state)
        // Add CORS layer
        .layer(cors)
        // Serve static files - Angular frontend (as fallback for assets etc)
        .fallback_service(serve_dir);
    
    let addr = SocketAddr::from(([127, 0, 0, 1], frontend_port));
    
    let listener = TcpListener::bind(addr)
        .await
        .map_err(|e| format!("Failed to bind to port {}: {}", frontend_port, e))?;
    
    info!("Frontend server listening on http://localhost:{}", frontend_port);
    
    // Store the server handle for graceful shutdown
    let handle = tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, app).await {
            error!("Frontend server error: {}", e);
        }
    });
    
    let mut guard = get_handle().await.lock().await;
    *guard = Some(handle);
    
    Ok(())
}

/// Serve index.html with injected CSS for native macOS header
async fn serve_index(
    State(state): State<Arc<FrontendServerState>>,
) -> impl IntoResponse {
    let index_path = state.frontend_dir.join("index.html");
    
    match tokio::fs::read_to_string(&index_path).await {
        Ok(html) => {
            // Add Cache-Control header to prevent caching old index.html
            let headers = [
                ("Content-Type", "text/html"),
                ("Cache-Control", "no-cache, no-store, must-revalidate"),
                ("Pragma", "no-cache"),
                ("Expires", "0"),
            ];
            
            (StatusCode::OK, headers, html).into_response()
        },
        Err(e) => {
            error!("Failed to read index.html: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, "Failed to load application").into_response()
        }
    }
}

/// Stop the frontend server
pub async fn stop() -> Result<(), String> {
    let mut guard = get_handle().await.lock().await;
    if let Some(handle) = guard.take() {
        handle.abort();
        info!("Frontend server stopped");
    }
    Ok(())
}

/// Proxy handler for /api/* and /actuator/* requests
async fn proxy_handler(
    State(state): State<Arc<FrontendServerState>>,
    req: Request,
) -> Response {
    let uri = req.uri().clone();
    let method = req.method().clone();
    let headers = req.headers().clone();
    
    // Build the backend URL
    let path = uri.path_and_query().map(|pq| pq.as_str()).unwrap_or("/");
    let backend_url = format!("http://127.0.0.1:{}{}", state.backend_port, path);
    
    debug!("Proxying {} {} -> {}", method, uri.path(), backend_url);
    
    // Get Content-Type from request headers
    let content_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    
    // Get Authorization header for JWT token
    let authorization = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    
    // Read the request body
    let body_bytes = match axum::body::to_bytes(req.into_body(), 100 * 1024 * 1024).await {
        Ok(bytes) => bytes,
        Err(e) => {
            error!("Failed to read request body: {}", e);
            return (StatusCode::BAD_REQUEST, format!("Failed to read request body: {}", e)).into_response();
        }
    };
    
    // Create reqwest client and request
    let client = reqwest::Client::builder()
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());
    
    let reqwest_method = match method.as_str() {
        "GET" => reqwest::Method::GET,
        "POST" => reqwest::Method::POST,
        "PUT" => reqwest::Method::PUT,
        "DELETE" => reqwest::Method::DELETE,
        "PATCH" => reqwest::Method::PATCH,
        "OPTIONS" => reqwest::Method::OPTIONS,
        "HEAD" => reqwest::Method::HEAD,
        _ => reqwest::Method::GET,
    };
    
    let mut backend_req = client.request(reqwest_method, &backend_url);
    
    // Set Content-Type if present in original request
    if let Some(ct) = &content_type {
        backend_req = backend_req.header("Content-Type", ct);
    }
    
    // Set Authorization header if present (for JWT authentication)
    if let Some(auth) = &authorization {
        backend_req = backend_req.header("Authorization", auth);
    }
    
    // Add body if present
    if !body_bytes.is_empty() {
        backend_req = backend_req.body(body_bytes.to_vec());
    }
    
    // Send request to backend
    match backend_req.send().await {
        Ok(resp) => {
            let status = StatusCode::from_u16(resp.status().as_u16())
                .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
            
            // Get Content-Type from response
            let response_content_type = resp
                .headers()
                .get("content-type")
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_string())
                .unwrap_or_else(|| "application/octet-stream".to_string());
            
            debug!("Backend responded with status: {}, content-type: {}", status, response_content_type);
            
            // Get response body
            match resp.bytes().await {
                Ok(body) => {
                    let mut response = Response::builder()
                        .status(status)
                        .header("Content-Type", &response_content_type)
                        .body(Body::from(body.to_vec()))
                        .unwrap_or_else(|_| Response::new(Body::empty()));
                    
                    // Add CORS headers
                    response.headers_mut().insert(
                        header::ACCESS_CONTROL_ALLOW_ORIGIN,
                        "*".parse().unwrap()
                    );
                    
                    response
                }
                Err(e) => {
                    error!("Failed to read backend response: {}", e);
                    (StatusCode::BAD_GATEWAY, format!("Failed to read backend response: {}", e)).into_response()
                }
            }
        }
        Err(e) => {
            error!("Backend proxy error: {}", e);
            (StatusCode::BAD_GATEWAY, format!("Backend unavailable: {}", e)).into_response()
        }
    }
}

/// WebSocket proxy handler - upgrades connection and proxies to backend
async fn ws_proxy_handler(
    State(state): State<Arc<FrontendServerState>>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    let backend_port = state.backend_port;
    
    // Accept the WebSocket upgrade and handle the connection
    ws.on_upgrade(move |socket| async move {
        if let Err(e) = handle_ws_proxy(socket, backend_port).await {
            error!("WebSocket proxy error: {}", e);
        }
    })
}

/// Handle WebSocket proxying between client and backend
async fn handle_ws_proxy(client_socket: WebSocket, backend_port: u16) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let backend_url = format!("ws://127.0.0.1:{}/ws", backend_port);
    
    info!("Proxying WebSocket to: {}", backend_url);
    
    // Connect to the backend WebSocket
    let (backend_socket, _) = connect_async(&backend_url).await?;
    
    info!("Connected to backend WebSocket");
    
    // Split both sockets into sender and receiver halves
    let (mut client_tx, mut client_rx) = client_socket.split();
    let (mut backend_tx, mut backend_rx) = backend_socket.split();
    
    // Spawn task to forward messages from client to backend
    let client_to_backend = tokio::spawn(async move {
        while let Some(msg) = client_rx.next().await {
            match msg {
                Ok(msg) => {
                    // Convert axum Message to tungstenite Message
                    let tung_msg = match msg {
                        Message::Text(text) => TungsteniteMessage::Text(text.to_string()),
                        Message::Binary(data) => TungsteniteMessage::Binary(data.to_vec()),
                        Message::Ping(data) => TungsteniteMessage::Ping(data.to_vec()),
                        Message::Pong(data) => TungsteniteMessage::Pong(data.to_vec()),
                        Message::Close(frame) => {
                            if let Some(cf) = frame {
                                TungsteniteMessage::Close(Some(tokio_tungstenite::tungstenite::protocol::CloseFrame {
                                    code: tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode::from(cf.code),
                                    reason: cf.reason.to_string().into(),
                                }))
                            } else {
                                TungsteniteMessage::Close(None)
                            }
                        }
                    };
                    
                    if let Err(e) = backend_tx.send(tung_msg).await {
                        warn!("Failed to send to backend: {}", e);
                        break;
                    }
                }
                Err(e) => {
                    warn!("Client WebSocket error: {}", e);
                    break;
                }
            }
        }
        let _ = backend_tx.close().await;
    });
    
    // Spawn task to forward messages from backend to client
    let backend_to_client = tokio::spawn(async move {
        while let Some(msg) = backend_rx.next().await {
            match msg {
                Ok(msg) => {
                    // Convert tungstenite Message to axum Message
                    let axum_msg = match msg {
                        TungsteniteMessage::Text(text) => Message::Text(text.into()),
                        TungsteniteMessage::Binary(data) => Message::Binary(data.into()),
                        TungsteniteMessage::Ping(data) => Message::Ping(data.into()),
                        TungsteniteMessage::Pong(data) => Message::Pong(data.into()),
                        TungsteniteMessage::Close(frame) => {
                            if let Some(cf) = frame {
                                Message::Close(Some(axum::extract::ws::CloseFrame {
                                    code: cf.code.into(),
                                    reason: cf.reason.to_string().into(),
                                }))
                            } else {
                                Message::Close(None)
                            }
                        }
                        TungsteniteMessage::Frame(_) => continue, // Skip raw frames
                    };
                    
                    if let Err(e) = client_tx.send(axum_msg).await {
                        warn!("Failed to send to client: {}", e);
                        break;
                    }
                }
                Err(e) => {
                    warn!("Backend WebSocket error: {}", e);
                    break;
                }
            }
        }
        let _ = client_tx.close().await;
    });
    
    // Wait for either direction to complete
    tokio::select! {
        _ = client_to_backend => {
            debug!("Client to backend task completed");
        }
        _ = backend_to_client => {
            debug!("Backend to client task completed");
        }
    }
    
    info!("WebSocket proxy connection closed");
    Ok(())
}
