// System Tray Module
// Handles macOS menubar icon and menu

use tauri::{
    App, AppHandle, Manager,
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::{TrayIconBuilder, TrayIconEvent, MouseButton, MouseButtonState},
    image::Image,
};
use tracing::{info, error};

/// Setup system tray
pub fn setup(app: &App) -> Result<(), Box<dyn std::error::Error>> {
    // Create menu items
    let open_item = MenuItem::with_id(app, "open", "Open BookLore", true, None::<&str>)?;
    let separator1 = PredefinedMenuItem::separator(app)?;
    let restart_item = MenuItem::with_id(app, "restart", "Restart Services", true, None::<&str>)?;
    let separator2 = PredefinedMenuItem::separator(app)?;
    let autostart_item = MenuItem::with_id(app, "autostart", "Launch at Login", true, None::<&str>)?;
    let separator3 = PredefinedMenuItem::separator(app)?;
    let about_item = MenuItem::with_id(app, "about", "About BookLore", true, None::<&str>)?;
    let quit_item = MenuItem::with_id(app, "quit", "Quit BookLore", true, None::<&str>)?;
    
    // Build menu
    let menu = Menu::with_items(app, &[
        &open_item,
        &separator1,
        &restart_item,
        &separator2,
        &autostart_item,
        &separator3,
        &about_item,
        &quit_item,
    ])?;
    
    // Create tray icon
    // Using a simple emoji as fallback - in production, use proper icon
    let _tray = TrayIconBuilder::new()
        .icon(get_tray_icon(app)?)
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| {
            handle_menu_event(app, event.id.as_ref());
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                let app = tray.app_handle();
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
        })
        .build(app)?;
    
    info!("System tray initialized");
    Ok(())
}

/// Handle menu item clicks
fn handle_menu_event(app: &AppHandle, menu_id: &str) {
    match menu_id {
        "open" => {
            // Open in browser
            // Open in browser
            let _ = open::that(format!("http://localhost:{}", crate::constants::FRONTEND_PORT));
        }
        "restart" => {
            // Restart services
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                // Get state and restart
                let state = app.state::<crate::AppState>();
                if let Err(e) = crate::stop_services(state.clone()).await {
                    error!("Failed to stop services: {}", e);
                }
                if let Err(e) = crate::start_services(app.clone(), state).await {
                    error!("Failed to restart services: {}", e);
                }
            });
        }
        "autostart" => {
            // Toggle autostart
            info!("Autostart toggled");
            // This is managed by tauri-plugin-autostart
        }
        "about" => {
            // Show about dialog
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.set_focus();
            }
        }
        "quit" => {
            // Quit application
            info!("Quit requested");
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                // Stop services before quitting
                let state = app.state::<crate::AppState>();
                let _ = crate::stop_services(state).await;
                app.exit(0);
            });
        }
        _ => {}
    }
}

/// Get tray icon
fn get_tray_icon(app: &App) -> Result<Image<'static>, Box<dyn std::error::Error>> {
    // Try to load icon from resources
    let icon_path = if cfg!(debug_assertions) {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("icons")
            .join("icon.png")
    } else {
        app.path().resource_dir()?
            .join("icons")
            .join("icon.png")
    };
    
    if icon_path.exists() {
        // Load icon file and decode it
        use image::GenericImageView;
        let img = image::open(&icon_path)?;
        let (width, height) = img.dimensions();
        let rgba = img.into_rgba8().into_raw();
        Ok(Image::new_owned(rgba, width, height))
    } else {
        // Fallback: create a simple colored icon
        // 16x16 RGBA image (book emoji color)
        let size = 16;
        let mut pixels = vec![0u8; size * size * 4];
        
        // Draw a simple book shape (brown background)
        for y in 0..size {
            for x in 0..size {
                let idx = (y * size + x) * 4;
                // Simple book icon - brown rectangle
                if (2..14).contains(&x) && (2..14).contains(&y) {
                    pixels[idx] = 139;     // R
                    pixels[idx + 1] = 90;  // G
                    pixels[idx + 2] = 43;  // B
                    pixels[idx + 3] = 255; // A
                } else {
                    // Transparent
                    pixels[idx + 3] = 0;
                }
            }
        }
        
        Ok(Image::new_owned(pixels, size as u32, size as u32))
    }
}

/// Update tray icon based on status
#[allow(dead_code)]
pub fn update_status(_app: &AppHandle, running: bool) {
    // Could update icon to show running/stopped status
    // For now, just log
    info!("Tray status updated: running={}", running);
}
