//! Appearance commands for window vibrancy/transparency effects.

/// Apply or remove native window vibrancy effect.
///
/// - `mode`: `"native"` to enable, `"off"` or `"css"` to disable
///
/// On macOS: uses NSVisualEffectView with `.sidebar` material.
/// On Windows: uses Mica effect (Windows 11) or Acrylic fallback.
/// On Linux: no-op (not supported natively).
#[tauri::command]
pub async fn set_window_vibrancy(
    window: tauri::Window,
    mode: String,
) -> Result<(), String> {
    let enabled = mode == "native";

    #[cfg(target_os = "macos")]
    {
        use window_vibrancy::{apply_vibrancy, NSVisualEffectMaterial};
        if enabled {
            apply_vibrancy(&window, NSVisualEffectMaterial::Sidebar, None, None)
                .map_err(|e| format!("Failed to apply vibrancy: {e}"))?;
        }
        // Disabling vibrancy on macOS requires re-applying an opaque background;
        // this is handled by the frontend via CSS when mode != "native".
    }

    #[cfg(target_os = "windows")]
    {
        if enabled {
            // Try Mica first (Windows 11), fall back to Acrylic
            use window_vibrancy::apply_mica;
            apply_mica(&window, None)
                .or_else(|_| {
                    use window_vibrancy::apply_acrylic;
                    apply_acrylic(&window, None)
                })
                .map_err(|e| format!("Failed to apply vibrancy: {e}"))?;
        }
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = (&window, &mode);
        if enabled {
            return Err("Native vibrancy is not supported on this platform".into());
        }
    }

    Ok(())
}
