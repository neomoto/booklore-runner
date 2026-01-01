// BookLore Runner - Main Entry Point
// Native macOS wrapper for BookLore using Tauri

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod jre;
mod mariadb;
mod backend;
mod tray;
mod frontend;
mod constants;

use std::sync::Arc;
use tauri::{Emitter, Manager, State};
#[cfg(target_os = "macos")]
use window_vibrancy::{apply_vibrancy, NSVisualEffectMaterial, NSVisualEffectState};
use tokio::sync::Mutex;
use tracing::{info, error, Level};
use tracing_subscriber::FmtSubscriber;

pub use booklore_runner_lib::*;

/// Application state shared across commands
pub struct AppState {
    pub mariadb_running: Arc<Mutex<bool>>,
    pub backend_running: Arc<Mutex<bool>>,
    pub frontend_running: Arc<Mutex<bool>>,
    pub jre_path: Arc<Mutex<Option<String>>>,
    pub backend_port: u16,
    pub frontend_port: u16,
    pub is_shutting_down: Arc<std::sync::atomic::AtomicBool>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            mariadb_running: Arc::new(Mutex::new(false)),
            backend_running: Arc::new(Mutex::new(false)),
            frontend_running: Arc::new(Mutex::new(false)),
            jre_path: Arc::new(Mutex::new(None)),
            backend_port: constants::BACKEND_PORT,
            frontend_port: constants::FRONTEND_PORT,
            is_shutting_down: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }
}

/// Status update payload sent to frontend
#[derive(Clone, serde::Serialize)]
pub struct StartupStatus {
    pub stage: String,      // "mariadb", "jre", "backend"
    pub status: String,     // "pending", "active", "complete", "error"
    pub message: String,
    pub progress: u8,
}

/// Emit status update to frontend
fn emit_status(app: &tauri::AppHandle, stage: &str, status: &str, message: &str, progress: u8) {
    let payload = StartupStatus {
        stage: stage.to_string(),
        status: status.to_string(),
        message: message.to_string(),
        progress,
    };
    
    if let Err(e) = app.emit("startup-status", payload) {
        error!("Failed to emit status: {}", e);
    }
}

/// Start all services (MariaDB, JRE check, Backend)
/// Start all services (MariaDB, JRE check, Backend)
#[tauri::command]
async fn start_services(app: tauri::AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    info!("Starting BookLore services...");
    
    // Step 1: Start Independent Services (MariaDB, JRE, Frontend) concurrently
    emit_status(&app, "mariadb", "active", "Starting database...", 10);
    emit_status(&app, "jre", "active", "Checking Java runtime...", 10);
    
    // Get frontend directory for frontend start
    let frontend_dir = app.path()
        .resource_dir()
        .map_err(|e| format!("Failed to get resource dir: {}", e))?
        .join("resources")
        .join("frontend");

    // Launch tasks in parallel
    let mariadb_future = mariadb::start(&app);
    let jre_future = jre::ensure_jre(&app);
    let frontend_future = frontend::start(state.frontend_port, state.backend_port, frontend_dir);
    
    let (mariadb_res, jre_res, frontend_res) = tokio::join!(mariadb_future, jre_future, frontend_future);
    
    // Handle MariaDB result
    match mariadb_res {
        Ok(_) => {
            *state.mariadb_running.lock().await = true;
            emit_status(&app, "mariadb", "complete", "Database ready", 30);
        }
        Err(e) => {
            emit_status(&app, "mariadb", "error", &format!("Database error: {}", e), 30);
            return Err(e);
        }
    }
    
    // Handle JRE result
    let jre_path = match jre_res {
        Ok(path) => {
            *state.jre_path.lock().await = Some(path.clone());
            emit_status(&app, "jre", "complete", "Java runtime ready", 60);
            path
        }
        Err(e) => {
            emit_status(&app, "jre", "error", &format!("JRE error: {}", e), 60);
            return Err(e);
        }
    };
    
    // Handle Frontend result
    match frontend_res {
        Ok(_) => {
            *state.frontend_running.lock().await = true;
            info!("Frontend server started on port {}", state.frontend_port);
        }
        Err(e) => {
            error!("Frontend server error: {}", e);
            // Don't fail - frontend issues shouldn't block backend access
        }
    }
    
    // Step 2: Start Backend (Dependencies ready)
    emit_status(&app, "backend", "active", "Starting BookLore backend...", 70);
    
    match backend::start(&app, &jre_path, state.backend_port).await {
        Ok(_) => {
            *state.backend_running.lock().await = true;
            emit_status(&app, "backend", "complete", "Backend ready", 85);
        }
        Err(e) => {
            emit_status(&app, "backend", "error", &format!("Backend error: {}", e), 100);
            return Err(e);
        }
    }
    
    emit_status(&app, "backend", "complete", "BookLore is ready!", 100);
    info!("All services started successfully. Open http://localhost:{}", state.frontend_port);
    Ok(())
}

/// Stop all services gracefully
#[tauri::command]
async fn stop_services(state: State<'_, AppState>) -> Result<(), String> {
    info!("Stopping BookLore services...");
    
    // Stop frontend server first
    if *state.frontend_running.lock().await {
        frontend::stop().await?;
        *state.frontend_running.lock().await = false;
    }
    
    // Stop backend
    if *state.backend_running.lock().await {
        backend::stop().await?;
        *state.backend_running.lock().await = false;
    }
    
    // Then stop MariaDB
    if *state.mariadb_running.lock().await {
        mariadb::stop().await?;
        *state.mariadb_running.lock().await = false;
    }
    
    info!("All services stopped");
    Ok(())
}

/// Open BookLore UI in default browser
#[tauri::command]
async fn open_ui(state: State<'_, AppState>) -> Result<(), String> {
    let url = format!("http://localhost:{}", state.frontend_port);
    open::that(&url).map_err(|e| e.to_string())?;
    Ok(())
}

/// Handle dropped files by copying them to bookdrop directory
#[tauri::command]
async fn handle_dropped_files(files: Vec<String>) -> Result<usize, String> {
    let bookdrop_dir = get_app_data_dir().join("bookdrop");
    
    // Ensure bookdrop directory exists
    if !bookdrop_dir.exists() {
        std::fs::create_dir_all(&bookdrop_dir)
            .map_err(|e| format!("Failed to create bookdrop directory: {}", e))?;
    }
    
    let mut count = 0;
    
    for file_path in files {
        let path = std::path::Path::new(&file_path);
        if let Some(file_name) = path.file_name() {
            let target_path = bookdrop_dir.join(file_name);
            match std::fs::copy(path, &target_path) {
                Ok(_) => {
                    info!("Imported file: {:?}", file_name);
                    count += 1;
                },
                Err(e) => error!("Failed to copy file {:?}: {}", file_name, e),
            }
        }
    }
    
    Ok(count)
}

/// Get app data directory path
pub fn get_app_data_dir() -> std::path::PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("BookLore")
}

fn main() {
    // Initialize logging
    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::INFO)
        .with_target(false)
        .finish();
    
    tracing::subscriber::set_global_default(subscriber)
        .expect("Failed to set tracing subscriber");
    
    info!("BookLore Runner starting...");
    
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--minimized"]),
        ))
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(AppState::default())
        .setup(|app| {
            // Create app data directory
            let data_dir = get_app_data_dir();
            std::fs::create_dir_all(&data_dir)
                .expect("Failed to create app data directory");
            
            info!("App data directory: {:?}", data_dir);
            
            // Setup system tray
            tray::setup(app)?;
            
            // Apply Vibrancy (native blur)
            #[cfg(target_os = "macos")]
            {
                let window = app.get_webview_window("main").unwrap();
                apply_vibrancy(
                    &window, 
                    NSVisualEffectMaterial::UnderWindowBackground, 
                    Some(NSVisualEffectState::Active), 
                    Some(10.0)
                ).expect("Unsupported platform! 'apply_vibrancy' is only supported on macOS");
            }
            
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            start_services,
            stop_services,
            open_ui,
            handle_dropped_files,
        ])
        .menu(|handle| {
            let menu = tauri::menu::Menu::new(handle)?;
            
            // App Menu
            let app_menu = tauri::menu::Submenu::new(handle, "BookLore", true)?;
            let about = tauri::menu::MenuItem::new(handle, "About BookLore", true, None::<&str>)?;
            let separator = tauri::menu::PredefinedMenuItem::separator(handle)?;
            let settings = tauri::menu::MenuItem::new(handle, "Settings...", true, Some("CmdOrCtrl+,"))?;
            let separator2 = tauri::menu::PredefinedMenuItem::separator(handle)?;
            // Custom Quit Item with ID
            let quit = tauri::menu::MenuItem::with_id(handle, "quit", "Quit BookLore", true, Some("CmdOrCtrl+Q"))?;
            
            app_menu.append_items(&[&about, &separator, &settings, &separator2, &quit])?;
             
            // Edit Menu
            let edit_menu = tauri::menu::Submenu::new(handle, "Edit", true)?;
            let undo = tauri::menu::PredefinedMenuItem::undo(handle, None)?;
            let redo = tauri::menu::PredefinedMenuItem::redo(handle, None)?;
            let cut = tauri::menu::PredefinedMenuItem::cut(handle, None)?;
            let copy = tauri::menu::PredefinedMenuItem::copy(handle, None)?;
            let paste = tauri::menu::PredefinedMenuItem::paste(handle, None)?;
            let select_all = tauri::menu::PredefinedMenuItem::select_all(handle, None)?;
            edit_menu.append_items(&[&undo, &redo, &cut, &copy, &paste, &select_all])?;

            // Window Menu
            let window_menu = tauri::menu::Submenu::new(handle, "Window", true)?;
            let minimize = tauri::menu::PredefinedMenuItem::minimize(handle, None)?;
            let zoom = tauri::menu::MenuItem::new(handle, "Zoom", true, None::<&str>)?;
            let close = tauri::menu::MenuItem::with_id(handle, "close", "Close Window", true, Some("CmdOrCtrl+W"))?;
            window_menu.append_items(&[&minimize, &zoom, &close])?;

            menu.append_items(&[&app_menu, &edit_menu, &window_menu])?;
            
            Ok(menu)
        })
        .on_menu_event(|app, event| {
             let id = event.id();
             if id.as_ref() == "quit" {
                 trigger_shutdown(app);
             } else if id.as_ref() == "close" {
                 if let Some(window) = app.get_webview_window("main") {
                     let _ = window.hide();
                 }
             }
        })
        .on_window_event(|window, event| {
            // Handle window close - minimize to tray instead
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                // Ensure Close also triggers hide, just to be safe (though default prevents close)
                window.hide().unwrap();
                api.prevent_close();
            }
        })
        .build(tauri::generate_context!())
        .expect("Error while building BookLore")
        .run(|app_handle, event| {
            match event {
                tauri::RunEvent::ExitRequested { api, .. } => {
                    let state = app_handle.state::<AppState>();
                    
                    // Only prevent exit if we haven't started the sequence yet
                    if !state.is_shutting_down.load(std::sync::atomic::Ordering::SeqCst) {
                        api.prevent_exit();
                        trigger_shutdown(app_handle);
                    } else {
                         // Allow exit to proceed (this happens when app_handle.exit(0) is called)
                         info!("Allowing exit to proceed");
                    }
                }
                tauri::RunEvent::Exit => {
                    info!("App exiting - performing safety cleanup");
                    
                    // Hide window immediately to prevent "frozen" look while main thread is blocked
                    if let Some(window) = app_handle.get_webview_window("main") {
                        let _ = window.hide();
                    }
                    
                    // Safety net: blocking cleanup
                    std::thread::spawn(move || {
                        let rt = tokio::runtime::Runtime::new().unwrap();
                        rt.block_on(async {
                            info!("Running safety cleanup...");
                            let _ = backend::stop().await;
                            let _ = mariadb::stop().await;
                            let _ = frontend::stop().await;
                        });
                    }).join().ok();
                }
                _ => {}
            }
        });
}

/// Trigger the graceful shutdown sequence
fn trigger_shutdown(app_handle: &tauri::AppHandle) {
    let state = app_handle.state::<AppState>();
    
    // Check if we are already shutting down
    // Atomic check is safe
    if !state.is_shutting_down.load(std::sync::atomic::Ordering::SeqCst) {
        state.is_shutting_down.store(true, std::sync::atomic::Ordering::SeqCst);
        
        info!("Initiating graceful shutdown sequence...");
        
        let app_handle = app_handle.clone();
        
        // Show the window if it was hidden
        if let Some(window) = app_handle.get_webview_window("main") {
            let _ = window.show();
            let _ = window.set_focus();
            
            // Navigate to loader page to show progress with shutdown flag
            #[cfg(debug_assertions)]
            let url = "http://localhost:1420?shutdown=true";
            #[cfg(not(debug_assertions))]
            let url = "tauri://localhost?shutdown=true";
            
            // We need to navigate back to the wrapper UI
            let _ = window.eval(&format!("window.location.href = '{}'", url));
        }
        
        // Spawn shutdown task
        tauri::async_runtime::spawn(async move {
            // Re-fetch state from owned app handle
            let state = app_handle.state::<AppState>();
            // Give UI time to load
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            
            let _ = app_handle.emit("shutdown-start", ());
            
            // 1. Stop Backend
            let _ = app_handle.emit("shutdown-status", serde_json::json!({
                "stage": "backend", "status": "active", "message": "Stopping backend...", "progress": 20
            }));
            
            if *state.backend_running.lock().await {
                if let Err(e) = backend::stop().await {
                    error!("Failed to stop backend: {}", e);
                    let _ = app_handle.emit("shutdown-status", serde_json::json!({
                        "stage": "backend", "status": "error", "message": format!("Error: {}", e)
                    }));
                } else {
                    *state.backend_running.lock().await = false;
                }
            }
            
            let _ = app_handle.emit("shutdown-status", serde_json::json!({
                "stage": "backend", "status": "complete", "message": "Backend stopped", "progress": 50
            }));
            
            // 2. Stop MariaDB
            let _ = app_handle.emit("shutdown-status", serde_json::json!({
                "stage": "mariadb", "status": "active", "message": "Stopping database...", "progress": 60
            }));
            
            // Mark JRE as skipped/done since we don't manage it explicitly during stop
            let _ = app_handle.emit("shutdown-status", serde_json::json!({
                "stage": "jre", "status": "complete", "message": "Runtime stopped", "progress": 60
            }));
            
            if *state.mariadb_running.lock().await {
                if let Err(e) = mariadb::stop().await {
                    error!("Failed to stop MariaDB: {}", e);
                } else {
                    *state.mariadb_running.lock().await = false;
                }
            }
            
            let _ = app_handle.emit("shutdown-status", serde_json::json!({
                "stage": "mariadb", "status": "complete", "message": "Database stopped", "progress": 90
            }));
            
            // 3. Stop Frontend (last)
            if *state.frontend_running.lock().await {
                let _ = frontend::stop().await;
                *state.frontend_running.lock().await = false;
            }
            
            let _ = app_handle.emit("shutdown-status", serde_json::json!({
                "stage": "backend", "status": "complete", "message": "Goodnight!", "progress": 100
            }));
            
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            
            // Force exit
            app_handle.exit(0);
        });
    } else {
         info!("Shutdown already in progress");
    }
}
