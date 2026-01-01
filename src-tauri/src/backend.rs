// Spring Boot Backend Management Module
// Handles launching and monitoring the BookLore Java backend

use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::OnceLock;
use tauri::{AppHandle, Manager};
use tokio::sync::Mutex;
use tracing::info;

// Store the backend process handle
static BACKEND_PROCESS: OnceLock<Mutex<Option<Child>>> = OnceLock::new();

fn get_process_mutex() -> &'static Mutex<Option<Child>> {
    BACKEND_PROCESS.get_or_init(|| Mutex::new(None))
}

/// Get the BookLore JAR path
fn get_jar_path(app: &AppHandle) -> PathBuf {
    if cfg!(debug_assertions) {
        // Development: look in resources folder
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("resources")
            .join("booklore-api.jar")
    } else {
        // Production: look in app bundle Resources/resources
        app.path().resource_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join("resources")
            .join("booklore-api.jar")
    }
}

/// Get the frontend dist path
#[allow(dead_code)]
fn get_frontend_path(app: &AppHandle) -> PathBuf {
    if cfg!(debug_assertions) {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("resources")
            .join("frontend")
    } else {
        app.path().resource_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join("frontend")
    }
}

/// Get the books directory
fn get_books_dir() -> PathBuf {
    crate::get_app_data_dir().join("books")
}

/// Get the BookDrop directory
fn get_bookdrop_dir() -> PathBuf {
    crate::get_app_data_dir().join("bookdrop")
}

/// Start the BookLore Spring Boot backend
pub async fn start(app: &AppHandle, java_path: &str, port: u16) -> Result<(), String> {
    // Check if already running
    {
        let guard = get_process_mutex().lock().await;
        if guard.is_some() {
            info!("Backend already running");
            return Ok(());
        }
    }
    
    let jar_path = get_jar_path(app);
    
    if !jar_path.exists() {
        return Err(format!("BookLore JAR not found at {:?}", jar_path));
    }
    
    info!("Starting BookLore backend from {:?}", jar_path);
    
    // Create necessary directories
    let app_data_dir = crate::get_app_data_dir();
    let config_dir = app_data_dir.join("config");
    let books_dir = get_books_dir();
    let bookdrop_dir = get_bookdrop_dir();
    
    std::fs::create_dir_all(&config_dir).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&books_dir).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&bookdrop_dir).map_err(|e| e.to_string())?;
    
    // Build database URL - use TCP connection to localhost
    // Build database URL - use TCP connection to localhost
    let database_url = format!("jdbc:mariadb://127.0.0.1:{}/booklore?createDatabaseIfNotExist=true", crate::constants::MARIADB_PORT);
    
    // Get JAVA_HOME
    let java_home = crate::jre::get_java_home();
    
    // Build the command
    let child = Command::new(java_path)
        .env("JAVA_HOME", &java_home)
        .env("DATABASE_URL", &database_url)
        .env("DATABASE_USERNAME", "root")
        .env("DATABASE_PASSWORD", "")
        .env("BOOKLORE_PORT", port.to_string())
        .arg("-Xmx512m")  // Limit heap size
        .arg("-Xms128m")
        .arg(format!("-Dapp.path-config={}", config_dir.display()))
        .arg(format!("-Dapp.bookdrop-folder={}", bookdrop_dir.display()))
        .arg(format!("-Dserver.port={}", port))
        .arg("-jar")
        .arg(&jar_path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to start backend: {}", e))?;
    
    info!("Backend started with PID: {}", child.id());
    
    // Store process handle
    {
        let mut guard = get_process_mutex().lock().await;
        *guard = Some(child);
    }
    
    // Wait for health check
    wait_for_backend(port).await?;
    
    info!("Backend is ready on port {}", port);
    Ok(())
}

/// Stop the backend process
pub async fn stop() -> Result<(), String> {
    let mut guard = get_process_mutex().lock().await;
    
    if let Some(mut child) = guard.take() {
        info!("Stopping backend...");
        
        // Send SIGTERM for graceful shutdown
        #[cfg(unix)]
        {
            
            unsafe {
                libc::kill(child.id() as i32, libc::SIGTERM);
            }
        }
        
        // Wait for graceful shutdown
        tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
        
        // Force kill if still running
        let _ = child.kill();
        let _ = child.wait();
        
        info!("Backend stopped");
    }
    
    Ok(())
}

/// Wait for backend to be ready
async fn wait_for_backend(port: u16) -> Result<(), String> {
    let health_url = format!("http://localhost:{}/api/v1/healthcheck", port);
    let client = reqwest::Client::new();
    
    for i in 0..240 {  // Wait up to 120 seconds
        match client.get(&health_url).send().await {
            Ok(response) if response.status().is_success() => {
                info!("Backend health check passed after {} attempts", i + 1);
                return Ok(());
            }
            Ok(_response) => {
                // Got a response but not successful, might be starting up
                tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
            }
            Err(_) => {
                // Connection failed, still starting
                tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
            }
        }
    }
    
    Err("Timeout waiting for backend to start".to_string())
}

/// Check if backend is healthy
#[allow(dead_code)]
pub async fn is_healthy(port: u16) -> bool {
    let health_url = format!("http://localhost:{}/api/v1/healthcheck", port);
    let client = reqwest::Client::new();
    
    match client.get(&health_url).send().await {
        Ok(response) => response.status().is_success(),
        Err(_) => false,
    }
}
