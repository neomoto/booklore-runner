// MariaDB Embedded Management Module
// Handles installation and lifecycle of embedded MariaDB for local database

use std::path::PathBuf;
use std::process::{Child, Command};
use std::sync::OnceLock;
use tauri::{AppHandle, Manager};
use tokio::sync::Mutex;
use tracing::{info, warn, error};

// MariaDB version to use
// MariaDB version to use
// Value is now in constants.rs
const MARIADB_VERSION: &str = crate::constants::MARIADB_VERSION;

// Store the MariaDB process handle
static MARIADB_PROCESS: OnceLock<Mutex<Option<Child>>> = OnceLock::new();

fn get_process_mutex() -> &'static Mutex<Option<Child>> {
    MARIADB_PROCESS.get_or_init(|| Mutex::new(None))
}

/// Get MariaDB installation directory
fn get_mariadb_dir() -> PathBuf {
    crate::get_app_data_dir().join("mariadb")
}

/// Get MariaDB data directory
fn get_data_dir() -> PathBuf {
    crate::get_app_data_dir().join("data")
}

/// Get MariaDB socket path
pub fn get_socket_path() -> PathBuf {
    crate::get_app_data_dir().join("mysql.sock")
}

/// Get MariaDB binary path
fn get_mariadbd_path() -> PathBuf {
    // First check for system MariaDB (Homebrew)
    if let Some(system_path) = find_system_mariadbd() {
        return PathBuf::from(system_path);
    }
    get_mariadb_dir().join("bin/mariadbd")
}

/// Find system mariadbd from Homebrew installation
fn find_system_mariadbd() -> Option<String> {
    // Try brew --prefix mariadb
    if let Ok(output) = Command::new("brew")
        .args(["--prefix", "mariadb"])
        .output()
    {
        if output.status.success() {
            let prefix = String::from_utf8_lossy(&output.stdout).trim().to_string();
            let mariadbd_path = format!("{}/bin/mariadbd", prefix);
            if std::path::Path::new(&mariadbd_path).exists() {
                info!("Found system MariaDB at: {}", mariadbd_path);
                return Some(mariadbd_path);
            }
        }
    }
    
    // Try common Homebrew paths
    let homebrew_paths = [
        "/opt/homebrew/opt/mariadb/bin/mariadbd",
        "/usr/local/opt/mariadb/bin/mariadbd",
    ];
    
    for path in &homebrew_paths {
        if std::path::Path::new(path).exists() {
            info!("Found system MariaDB at: {}", path);
            return Some(path.to_string());
        }
    }
    
    None
}

/// Get system MariaDB base directory (for share files etc)
fn get_system_mariadb_dir() -> Option<PathBuf> {
    // Try brew --prefix mariadb first
    if let Ok(output) = Command::new("brew")
        .args(["--prefix", "mariadb"])
        .output()
    {
        if output.status.success() {
            let prefix = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !prefix.is_empty() && std::path::Path::new(&prefix).exists() {
                return Some(PathBuf::from(prefix));
            }
        }
    }
    
    // Fallback to common Homebrew installation paths (for sandboxed app bundles)
    let homebrew_paths = [
        "/opt/homebrew/opt/mariadb",     // Apple Silicon
        "/usr/local/opt/mariadb",         // Intel
    ];
    
    for path in &homebrew_paths {
        let path_buf = PathBuf::from(path);
        if path_buf.join("bin/mariadbd").exists() {
            info!("Found system MariaDB dir at: {}", path);
            return Some(path_buf);
        }
    }
    
    None
}

/// Get mysql_install_db path
#[allow(dead_code)]
fn get_install_db_path() -> PathBuf {
    // Prefer system install-db
    if let Some(sys_dir) = get_system_mariadb_dir() {
        let sys_install_db = sys_dir.join("bin/mariadb-install-db");
        if sys_install_db.exists() {
            return sys_install_db;
        }
    }
    get_mariadb_dir().join("scripts/mariadb-install-db")
}

/// Check if MariaDB is installed (system or local)
fn is_mariadb_installed() -> bool {
    find_system_mariadbd().is_some() || get_mariadb_dir().join("bin/mariadbd").exists()
}

/// Check if database is initialized
fn is_database_initialized() -> bool {
    get_data_dir().join("mysql").exists()
}

/// Kill any stale MariaDB processes using our data directory
fn kill_stale_mariadb_processes(data_dir: &std::path::Path) {
    // Use pgrep to find mariadbd processes
    let output = Command::new("pgrep")
        .arg("-f")
        .arg("mariadbd.*BookLore")
        .output();
    
    if let Ok(out) = output {
        if out.status.success() {
            let pids = String::from_utf8_lossy(&out.stdout);
            for pid_str in pids.trim().lines() {
                if let Ok(pid) = pid_str.trim().parse::<i32>() {
                    warn!("Found stale MariaDB process (PID: {}), killing it", pid);
                    #[cfg(unix)]
                    unsafe {
                        libc::kill(pid, libc::SIGTERM);
                    }
                }
            }
            // Wait a moment for processes to terminate
            std::thread::sleep(std::time::Duration::from_secs(2));
        }
    }
    
    // Also check for lock files without active process
    let aria_lock = data_dir.join("aria_log_control");
    if aria_lock.exists() {
        info!("Data directory contains lock files, checking if in use");
    }
}

/// Start MariaDB server
pub async fn start(app: &AppHandle) -> Result<(), String> {
    // Check if already running
    {
        let guard = get_process_mutex().lock().await;
        if guard.is_some() {
            info!("MariaDB already running");
            return Ok(());
        }
    }
    
    // Ensure MariaDB is installed
    if !is_mariadb_installed() {
        crate::emit_status(app, "mariadb", "active", "Installing database server...", 15);
        install_mariadb(app).await?;
    }
    
    // Initialize database if needed
    if !is_database_initialized() {
        crate::emit_status(app, "mariadb", "active", "Initializing database...", 20);
        initialize_database()?;
    }
    
    // Start MariaDB
    crate::emit_status(app, "mariadb", "active", "Starting database server...", 25);
    
    let data_dir = get_data_dir();
    let socket_path = get_socket_path();
    
    // Kill any stale MariaDB processes using our data directory
    // This can happen if the app crashed without proper cleanup
    kill_stale_mariadb_processes(&data_dir);
    
    // Clean up old socket if exists
    if socket_path.exists() {
        info!("Removing stale socket file");
        let _ = std::fs::remove_file(&socket_path);
    }
    
    // Determine correct basedir and binary
    let (mariadbd_path, basedir) = if let Some(sys_dir) = get_system_mariadb_dir() {
        let p = sys_dir.join("bin/mariadbd");
        info!("Using System MariaDB at {:?} with basedir {:?}", p, sys_dir);
        (p, sys_dir)
    } else {
        let p = get_mariadbd_path();
        let b = get_mariadb_dir();
        info!("Using Bundled MariaDB at {:?} with basedir {:?}", p, b);
        (p, b)
    };

    let log_path = crate::get_app_data_dir().join("mariadb.log");
    info!("Redirecting MariaDB logs to {:?}", log_path);
    
    let log_file = std::fs::File::create(&log_path)
        .map_err(|e| format!("Failed to create log file: {}", e))?;
    let log_stderr = log_file.try_clone()
        .map_err(|e| format!("Failed to clone log file handle: {}", e))?;

    let child = Command::new(&mariadbd_path)
        .arg(format!("--basedir={}", basedir.display()))
        .arg(format!("--datadir={}", data_dir.display()))
        .arg(format!("--socket={}", socket_path.display()))
        .arg("--bind-address=127.0.0.1")  // Only localhost, no external access
        .arg(format!("--port={}", crate::constants::MARIADB_PORT))
        .arg("--skip-grant-tables")  // Single user mode, no auth needed
        .stdout(log_file)
        .stderr(log_stderr)
        .spawn()
        .map_err(|e| format!("Failed to start MariaDB: {}", e))?;
    
    info!("MariaDB started with PID: {}", child.id());
    
    // Store process handle
    {
        let mut guard = get_process_mutex().lock().await;
        *guard = Some(child);
    }
    
    // Wait for socket to be ready
    wait_for_socket(&socket_path).await?;
    
    // Create booklore database if not exists
    create_database().await?;
    
    info!("MariaDB is ready");
    Ok(())
}

/// Stop MariaDB server
pub async fn stop() -> Result<(), String> {
    let mut guard = get_process_mutex().lock().await;
    
    if let Some(mut child) = guard.take() {
        info!("Stopping MariaDB...");
        
        // Try graceful shutdown via TCP first
        let mysql_path = get_system_mariadb_dir()
            .map(|d| d.join("bin/mariadb"))
            .unwrap_or_else(|| get_mariadb_dir().join("bin/mariadb"));
        let _ = Command::new(&mysql_path)
            .arg("-h")
            .arg("127.0.0.1")
            .arg("-P")
            .arg("13306")
            .arg("-e")
            .arg("SHUTDOWN")
            .output();
        
        // Wait a bit for graceful shutdown
        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
        
        // Force kill if still running
        let _ = child.kill();
        let _ = child.wait();
        
        // Clean up socket
        let _ = std::fs::remove_file(get_socket_path());
        
        info!("MariaDB stopped");
    }
    
    Ok(())
}

/// Install MariaDB binaries
async fn install_mariadb(app: &AppHandle) -> Result<(), String> {
    let mariadb_dir = get_mariadb_dir();
    
    // For now, we expect MariaDB to be bundled with the app
    // In production, this would be in the app's Resources folder
    
    // Check for bundled MariaDB
    let resource_path = if cfg!(debug_assertions) {
        // Development: look for pre-downloaded MariaDB
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("resources")
            .join("mariadb")
    } else {
        // Production: look in app bundle
        if let Ok(resource_dir) = app.path().resource_dir() {
            resource_dir.join("mariadb")
        } else {
            return Err("Cannot find resource directory".to_string());
        }
    };
    
    if resource_path.exists() {
        // Copy bundled MariaDB
        copy_dir_recursive(&resource_path, &mariadb_dir)?;
        
        // Make binaries executable
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let bin_dir = mariadb_dir.join("bin");
            if let Ok(entries) = std::fs::read_dir(&bin_dir) {
                for entry in entries.filter_map(|e| e.ok()) {
                    let mut perms = entry.metadata()
                        .map_err(|e| e.to_string())?
                        .permissions();
                    perms.set_mode(0o755);
                    std::fs::set_permissions(entry.path(), perms)
                        .map_err(|e| e.to_string())?;
                }
            }
        }
        
        return Ok(());
    }
    
    // If not bundled, download (for development)
    info!("Downloading MariaDB {} for macOS ARM64...", MARIADB_VERSION);
    crate::emit_status(app, "mariadb", "active", "Downloading database server...", 15);
    
    // MariaDB download URL for macOS ARM64
    let download_url = format!(
        "https://archive.mariadb.org/mariadb-{}/bintar-darwin-arm64/mariadb-{}-darwin-arm64.tar.gz",
        MARIADB_VERSION, MARIADB_VERSION
    );
    
    let client = reqwest::Client::new();
    let response = client.get(&download_url)
        .send()
        .await
        .map_err(|e| format!("Failed to download MariaDB: {}", e))?;
    
    if !response.status().is_success() {
        return Err(format!("Download failed with status: {}", response.status()));
    }
    
    let bytes = response.bytes()
        .await
        .map_err(|e| format!("Failed to read response: {}", e))?;
    
    // Extract archive
    let temp_dir = std::env::temp_dir();
    let archive_path = temp_dir.join("mariadb-download.tar.gz");
    
    std::fs::write(&archive_path, &bytes)
        .map_err(|e| format!("Failed to write archive: {}", e))?;
    
    extract_mariadb(&archive_path, &mariadb_dir)?;
    
    let _ = std::fs::remove_file(&archive_path);
    
    info!("MariaDB installed to {:?}", mariadb_dir);
    Ok(())
}

/// Extract MariaDB archive
fn extract_mariadb(archive_path: &PathBuf, target_dir: &PathBuf) -> Result<(), String> {
    use flate2::read::GzDecoder;
    use tar::Archive;
    
    let file = std::fs::File::open(archive_path)
        .map_err(|e| format!("Failed to open archive: {}", e))?;
    
    let gz = GzDecoder::new(file);
    let mut archive = Archive::new(gz);
    
    let temp_extract = target_dir.parent()
        .ok_or("Invalid target")?
        .join("mariadb-extract-temp");
    
    std::fs::create_dir_all(&temp_extract)
        .map_err(|e| format!("Failed to create temp dir: {}", e))?;
    
    archive.unpack(&temp_extract)
        .map_err(|e| format!("Failed to extract: {}", e))?;
    
    // Find extracted directory
    let entries = std::fs::read_dir(&temp_extract)
        .map_err(|e| format!("Failed to read temp dir: {}", e))?;
    
    let mariadb_extracted = entries
        .filter_map(|e| e.ok())
        .find(|e| e.file_name().to_string_lossy().contains("mariadb"))
        .ok_or("MariaDB directory not found")?;
    
    std::fs::rename(mariadb_extracted.path(), target_dir)
        .map_err(|e| format!("Failed to move MariaDB: {}", e))?;
    
    let _ = std::fs::remove_dir_all(&temp_extract);
    
    // Make executables
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let bin_dir = target_dir.join("bin");
        if let Ok(entries) = std::fs::read_dir(&bin_dir) {
            for entry in entries.filter_map(|e| e.ok()) {
                let mut perms = entry.metadata().map_err(|e| e.to_string())?.permissions();
                perms.set_mode(0o755);
                let _ = std::fs::set_permissions(entry.path(), perms);
            }
        }
    }
    
    Ok(())
}

/// Initialize MariaDB database
fn initialize_database() -> Result<(), String> {
    let data_dir = get_data_dir();
    
    std::fs::create_dir_all(&data_dir)
        .map_err(|e| format!("Failed to create data directory: {}", e))?;
    
    // Prefer system MariaDB if available
    if let Some(sys_dir) = get_system_mariadb_dir() {
        let sys_install_db = sys_dir.join("bin/mariadb-install-db");
        if sys_install_db.exists() {
            info!("Using system mariadb-install-db from {:?}", sys_install_db);
            return run_install_db(&sys_install_db, &sys_dir, &data_dir);
        }
    }
    
    // Fall back to local installation
    let mariadb_dir = get_mariadb_dir();
    
    // Run mariadb-install-db
    let install_db = mariadb_dir.join("scripts/mariadb-install-db");
    
    if !install_db.exists() {
        // Try alternate location
        let alt_install_db = mariadb_dir.join("bin/mariadb-install-db");
        if alt_install_db.exists() {
            return run_install_db(&alt_install_db, &mariadb_dir, &data_dir);
        }
        return Err("mariadb-install-db not found. Please install MariaDB via Homebrew: brew install mariadb".to_string());
    }
    
    run_install_db(&install_db, &mariadb_dir, &data_dir)
}

fn run_install_db(install_db: &std::path::Path, mariadb_dir: &std::path::Path, data_dir: &std::path::Path) -> Result<(), String> {
    let output = Command::new(install_db)
        .arg(format!("--basedir={}", mariadb_dir.display()))
        .arg(format!("--datadir={}", data_dir.display()))
        .arg("--auth-root-authentication-method=normal")
        .output()
        .map_err(|e| format!("Failed to run mariadb-install-db: {}", e))?;
    
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        error!("mariadb-install-db failed: {}", stderr);
        return Err(format!("Database initialization failed: {}", stderr));
    }
    
    info!("Database initialized successfully");
    Ok(())
}

/// Wait for MariaDB to be ready via TCP
async fn wait_for_socket(_socket_path: &std::path::Path) -> Result<(), String> {
    info!("Waiting for MariaDB to be ready (TCP port {})", crate::constants::MARIADB_PORT);
    
    for i in 0..60 {
        // Try to connect via TCP - prefer system mariadb client
        let mysql_path = get_system_mariadb_dir()
            .map(|d| d.join("bin/mariadb"))
            .unwrap_or_else(|| get_mariadb_dir().join("bin/mariadb"));
        
        let output = Command::new(&mysql_path)
            .arg("-h")
            .arg("127.0.0.1")
            .arg("-P")
            .arg(crate::constants::MARIADB_PORT.to_string())
            .arg("-e")
            .arg("SELECT 1")
            .output();
        
        match output {
            Ok(out) => {
                if out.status.success() {
                    info!("MariaDB ready and connection successful");
                    return Ok(());
                } else {
                    let stderr = String::from_utf8_lossy(&out.stderr);
                    if i % 5 == 0 {
                        warn!("Attempt {}: Connection failed: {}", i, stderr.trim());
                    }
                }
            },
            Err(e) => {
                if i % 5 == 0 {
                    warn!("Attempt {}: Failed to run mysql check: {}", i, e);
                }
            }
        }
        
        tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;
    }
    
    Err("Timeout waiting for MariaDB to start (TCP connection check failed)".to_string())
}

/// Create booklore database
async fn create_database() -> Result<(), String> {
    // Prefer system mariadb client
    let mysql_path = get_system_mariadb_dir()
        .map(|d| d.join("bin/mariadb"))
        .unwrap_or_else(|| get_mariadb_dir().join("bin/mariadb"));
    
    let output = Command::new(&mysql_path)
        .arg("-h")
        .arg("127.0.0.1")
        .arg("-P")
        .arg(crate::constants::MARIADB_PORT.to_string())
        .arg("-e")
        .arg("CREATE DATABASE IF NOT EXISTS booklore CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci")
        .output()
        .map_err(|e| format!("Failed to create database: {}", e))?;
    
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        warn!("Create database warning: {}", stderr);
    }
    
    info!("booklore database ready");
    Ok(())
}

/// Helper to copy directory recursively
fn copy_dir_recursive(src: &PathBuf, dst: &PathBuf) -> Result<(), String> {
    std::fs::create_dir_all(dst)
        .map_err(|e| format!("Failed to create directory: {}", e))?;
    
    for entry in std::fs::read_dir(src).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        let dest_path = dst.join(entry.file_name());
        
        if path.is_dir() {
            copy_dir_recursive(&path, &dest_path)?;
        } else {
            std::fs::copy(&path, &dest_path)
                .map_err(|e| format!("Failed to copy file: {}", e))?;
        }
    }
    
    Ok(())
}
