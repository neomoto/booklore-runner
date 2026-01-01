// JRE Download and Management Module
// Handles automatic download of Eclipse Temurin JRE 21 for macOS ARM64

use std::path::PathBuf;
use std::process::Command;
use tauri::AppHandle;
use tracing::info;

const JRE_VERSION: &str = crate::constants::JRE_VERSION;
const ADOPTIUM_API: &str = crate::constants::ADOPTIUM_API;

/// Get the JRE installation directory
fn get_jre_dir() -> PathBuf {
    crate::get_app_data_dir().join("jre")
}

/// Get the java executable path
fn get_java_executable() -> PathBuf {
    get_jre_dir().join("Contents/Home/bin/java")
}

/// Check if JRE is installed and working
fn is_jre_installed() -> bool {
    let java_path = get_java_executable();
    
    if !java_path.exists() {
        return false;
    }
    
    // Verify it works
    match Command::new(&java_path).arg("-version").output() {
        Ok(output) => output.status.success(),
        Err(_) => false,
    }
}

/// Check for system Java installation (macOS)
fn find_system_java() -> Option<String> {
    // Try /usr/libexec/java_home first (macOS standard)
    if let Ok(output) = Command::new("/usr/libexec/java_home")
        .arg("-v")
        .arg("21")
        .output()
    {
        if output.status.success() {
            let java_home = String::from_utf8_lossy(&output.stdout).trim().to_string();
            let java_path = format!("{}/bin/java", java_home);
            if std::path::Path::new(&java_path).exists() {
                info!("Found system Java 21 at: {}", java_path);
                return Some(java_path);
            }
        }
    }
    
    // Try JAVA_HOME environment variable
    if let Ok(java_home) = std::env::var("JAVA_HOME") {
        let java_path = format!("{}/bin/java", java_home);
        if std::path::Path::new(&java_path).exists() {
            // Verify it's Java 21+
            if let Ok(output) = Command::new(&java_path).arg("-version").output() {
                let version_str = String::from_utf8_lossy(&output.stderr);
                if version_str.contains("21.") || version_str.contains("22.") || version_str.contains("23.") || version_str.contains("24.") {
                    info!("Found JAVA_HOME Java at: {}", java_path);
                    return Some(java_path);
                }
            }
        }
    }
    
    // Try 'java' in PATH
    if let Ok(output) = Command::new("which").arg("java").output() {
        if output.status.success() {
            let java_path = String::from_utf8_lossy(&output.stdout).trim().to_string();
            // Verify it's Java 21+
            if let Ok(version_output) = Command::new(&java_path).arg("-version").output() {
                let version_str = String::from_utf8_lossy(&version_output.stderr);
                if version_str.contains("21.") || version_str.contains("22.") || version_str.contains("23.") || version_str.contains("24.") {
                    info!("Found PATH Java at: {}", java_path);
                    return Some(java_path);
                }
            }
        }
    }
    
    None
}

/// Download and install JRE if not present
pub async fn ensure_jre(app: &AppHandle) -> Result<String, String> {
    // Check our bundled/downloaded JRE first
    let java_path = get_java_executable();
    
    if is_jre_installed() {
        info!("JRE already installed at {:?}", java_path);
        crate::emit_status(app, "jre", "complete", "Using bundled Java", 50);
        return Ok(java_path.to_string_lossy().to_string());
    }

    // Then check for system Java 21+
    if let Some(system_java) = find_system_java() {
        crate::emit_status(app, "jre", "complete", "Using system Java", 50);
        return Ok(system_java);
    }
    
    info!("No system Java 21+ found, downloading...");
    download_jre(app).await?;
    
    if is_jre_installed() {
        Ok(java_path.to_string_lossy().to_string())
    } else {
        Err("JRE installation verification failed".to_string())
    }
}

/// Download JRE from Adoptium
async fn download_jre(app: &AppHandle) -> Result<(), String> {
    let jre_dir = get_jre_dir();
    
    // Clean up any partial installation
    if jre_dir.exists() {
        std::fs::remove_dir_all(&jre_dir)
            .map_err(|e| format!("Failed to clean JRE directory: {}", e))?;
    }
    
    // Adoptium API URL for macOS ARM64 JRE
    let download_url = format!(
        "{}/{}/ga/mac/aarch64/jre/hotspot/normal/eclipse",
        ADOPTIUM_API, JRE_VERSION
    );
    
    info!("Downloading JRE from: {}", download_url);
    
    // Emit download progress
    crate::emit_status(app, "jre", "active", "Downloading Java runtime...", 45);
    
    // Download the archive with redirect support
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::limited(10))
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {}", e))?;
        
    let response = client.get(&download_url)
        .send()
        .await
        .map_err(|e| format!("Failed to download JRE: {}", e))?;
    
    if !response.status().is_success() {
        return Err(format!("Download failed with status: {} - URL: {}", response.status(), download_url));
    }
    
    let total_size = response.content_length().unwrap_or(0);
    info!("Download size: {} bytes", total_size);
    
    // Create temp file for download
    let temp_dir = std::env::temp_dir();
    let archive_path = temp_dir.join("jre-download.tar.gz");
    
    let bytes = response.bytes()
        .await
        .map_err(|e| format!("Failed to read response: {}", e))?;
    
    std::fs::write(&archive_path, &bytes)
        .map_err(|e| format!("Failed to write archive: {}", e))?;
    
    info!("Downloaded {} bytes to {:?}", bytes.len(), archive_path);
    
    // Emit extraction progress
    crate::emit_status(app, "jre", "active", "Extracting Java runtime...", 55);
    
    // Extract the archive
    extract_jre(&archive_path, &jre_dir)?;
    
    // Clean up temp file
    let _ = std::fs::remove_file(&archive_path);
    
    info!("JRE installed successfully");
    Ok(())
}

/// Extract JRE tar.gz archive
fn extract_jre(archive_path: &PathBuf, target_dir: &PathBuf) -> Result<(), String> {
    use flate2::read::GzDecoder;
    use tar::Archive;
    
    let file = std::fs::File::open(archive_path)
        .map_err(|e| format!("Failed to open archive: {}", e))?;
    
    let gz = GzDecoder::new(file);
    let mut archive = Archive::new(gz);
    
    // Create parent directory
    let parent = target_dir.parent()
        .ok_or("Invalid target directory")?;
    std::fs::create_dir_all(parent)
        .map_err(|e| format!("Failed to create parent directory: {}", e))?;
    
    // Extract to temp directory first
    let temp_extract = parent.join("jre-extract-temp");
    std::fs::create_dir_all(&temp_extract)
        .map_err(|e| format!("Failed to create temp directory: {}", e))?;
    
    archive.unpack(&temp_extract)
        .map_err(|e| format!("Failed to extract archive: {}", e))?;
    
    // Find the extracted JDK directory (has a version in the name)
    let entries = std::fs::read_dir(&temp_extract)
        .map_err(|e| format!("Failed to read temp directory: {}", e))?;
    
    let jdk_dir = entries
        .filter_map(|e| e.ok())
        .find(|e| e.file_name().to_string_lossy().contains("jdk"))
        .ok_or("JDK directory not found in archive")?;
    
    // Move to final location
    std::fs::rename(jdk_dir.path(), target_dir)
        .map_err(|e| format!("Failed to move JRE directory: {}", e))?;
    
    // Clean up temp directory
    let _ = std::fs::remove_dir_all(&temp_extract);
    
    // Make java executable
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let java_path = target_dir.join("Contents/Home/bin/java");
        if java_path.exists() {
            let mut perms = std::fs::metadata(&java_path)
                .map_err(|e| format!("Failed to get permissions: {}", e))?
                .permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&java_path, perms)
                .map_err(|e| format!("Failed to set permissions: {}", e))?;
        }
    }
    
    Ok(())
}

/// Get JAVA_HOME path
pub fn get_java_home() -> PathBuf {
    get_jre_dir().join("Contents/Home")
}
