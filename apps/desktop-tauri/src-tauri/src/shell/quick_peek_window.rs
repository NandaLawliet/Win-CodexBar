//! Tooltip-sized, cursor-anchored usage peek, independent of the tray flyout.
//! The global shortcut toggles this window; clicking the tray still opens the flyout.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use tauri::{AppHandle, Manager, PhysicalPosition, WebviewUrl};

pub const LABEL: &str = "quick-peek";
const WIDTH: f64 = 140.0;
const HEIGHT: f64 = 134.0;
const CURSOR_GAP: i32 = 12;
const BLUR_GRACE: Duration = Duration::from_millis(250);

static REQUESTED_VISIBLE: AtomicBool = AtomicBool::new(false);
static LAST_SHOWN: Mutex<Option<Instant>> = Mutex::new(None);

#[cfg(target_os = "windows")]
#[repr(C)]
struct WinPoint {
    x: i32,
    y: i32,
}

#[cfg(target_os = "windows")]
#[link(name = "user32")]
unsafe extern "system" {
    fn GetCursorPos(point: *mut WinPoint) -> i32;
}

fn cursor_position() -> Option<(i32, i32)> {
    #[cfg(target_os = "windows")]
    {
        let mut point = WinPoint { x: 0, y: 0 };
        // SAFETY: GetCursorPos writes to this valid, initialized POINT pointer.
        if unsafe { GetCursorPos(&mut point) } != 0 {
            return Some((point.x, point.y));
        }
    }
    None
}

fn clamp_axis(value: i32, start: i32, extent: i32, window_extent: i32) -> i32 {
    let last = start.saturating_add(extent.saturating_sub(window_extent).max(0));
    value.clamp(start, last)
}

/// Prefer the lower-right of the pointer; flip independently at the edges.
/// Both window and work-area dimensions are physical pixels (DPI-safe).
fn position_near_cursor(
    cursor: (i32, i32),
    window_size: (i32, i32),
    work_area: (i32, i32, i32, i32),
) -> (i32, i32) {
    let (left, top, width, height) = work_area;
    let (win_w, win_h) = window_size;
    let right = left.saturating_add(width);
    let bottom = top.saturating_add(height);

    let x = if cursor.0.saturating_add(CURSOR_GAP).saturating_add(win_w) <= right {
        cursor.0.saturating_add(CURSOR_GAP)
    } else {
        cursor.0.saturating_sub(CURSOR_GAP).saturating_sub(win_w)
    };
    let y = if cursor.1.saturating_add(CURSOR_GAP).saturating_add(win_h) <= bottom {
        cursor.1.saturating_add(CURSOR_GAP)
    } else {
        cursor.1.saturating_sub(CURSOR_GAP).saturating_sub(win_h)
    };

    (
        clamp_axis(x, left, width, win_w),
        clamp_axis(y, top, height, win_h),
    )
}

fn place_at_pointer(window: &tauri::WebviewWindow) -> Result<(), String> {
    let monitors = window.available_monitors().map_err(|e| e.to_string())?;
    let cursor = cursor_position();
    let monitor = monitors
        .into_iter()
        .find(|monitor| {
            let Some((x, y)) = cursor else {
                return false;
            };
            let p = monitor.position();
            let s = monitor.size();
            x >= p.x
                && y >= p.y
                && x < p.x.saturating_add(s.width as i32)
                && y < p.y.saturating_add(s.height as i32)
        })
        .or_else(|| window.primary_monitor().ok().flatten())
        .ok_or_else(|| "No monitor available for Quick Peek".to_string())?;

    let area = super::geometry::monitor_work_area_rect(&monitor);
    let size = window.outer_size().map_err(|e| e.to_string())?;
    let cursor = cursor.unwrap_or((
        area.x.saturating_add(area.width as i32 / 2),
        area.y.saturating_add(area.height as i32 / 2),
    ));
    let (x, y) = position_near_cursor(
        cursor,
        (size.width as i32, size.height as i32),
        (area.x, area.y, area.width as i32, area.height as i32),
    );
    window
        .set_position(PhysicalPosition::new(x, y))
        .map_err(|e| e.to_string())
}

fn show(window: &tauri::WebviewWindow) -> Result<(), String> {
    place_at_pointer(window)?;
    if let Ok(mut last) = LAST_SHOWN.lock() {
        *last = Some(Instant::now());
    }
    window.show().map_err(|e| e.to_string())?;
    window.set_focus().map_err(|e| e.to_string())
}

fn open_or_focus(app: &AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window(LABEL) {
        return show(&window);
    }

    // First creation stays hidden until the React surface has loaded cache.
    // This avoids briefly showing the main app's bootstrap/loading UI.
    tauri::WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("index.html?window=quick-peek".into()))
        .title("CodexBar Quick Peek")
        .inner_size(WIDTH, HEIGHT)
        .decorations(false)
        .resizable(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .shadow(false)
        // WebView2 shares a process-level color-scheme profile. Pin its theme
        // like the float bar/flyout; the Quick Peek stylesheet stays white.
        .theme(Some(tauri::Theme::Dark))
        .visible(false)
        .build()
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Called from the existing Tauri global-shortcut handler.
pub fn toggle(app: &AppHandle) {
    if REQUESTED_VISIBLE.swap(true, Ordering::SeqCst) {
        let _ = hide(app);
    } else if let Err(err) = open_or_focus(app) {
        REQUESTED_VISIBLE.store(false, Ordering::SeqCst);
        tracing::warn!("Could not open Quick Peek: {err}");
    }
}

fn hide(app: &AppHandle) -> Result<(), String> {
    REQUESTED_VISIBLE.store(false, Ordering::SeqCst);
    if let Some(window) = app.get_webview_window(LABEL) {
        window.hide().map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Frontend signals that cached provider data was read. Safe to call twice
/// under React StrictMode; a shortcut toggled off during load stays off.
#[tauri::command]
pub fn reveal_quick_peek_window(app: AppHandle) -> Result<(), String> {
    if REQUESTED_VISIBLE.load(Ordering::SeqCst) {
        if let Some(window) = app.get_webview_window(LABEL) {
            show(&window)?;
        }
    }
    Ok(())
}

#[tauri::command]
pub fn dismiss_quick_peek_window(app: AppHandle) -> Result<(), String> {
    hide(&app)
}

/// Hooked before the main-window event path. Outside-click blur and Escape
/// both hide (never destroy) the cached WebView2 window.
pub fn handle_window_event(window: &tauri::Window, event: &tauri::WindowEvent) -> bool {
    if window.label() != LABEL {
        return false;
    }
    match event {
        tauri::WindowEvent::Focused(false) => {
            let in_grace = LAST_SHOWN
                .lock()
                .ok()
                .and_then(|last| *last)
                .is_some_and(|last| last.elapsed() < BLUR_GRACE);
            if !in_grace {
                let _ = hide(window.app_handle());
            }
        }
        tauri::WindowEvent::CloseRequested { api, .. } => {
            api.prevent_close();
            let _ = hide(window.app_handle());
        }
        _ => {}
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn popup_prefers_cursor_lower_right() {
        assert_eq!(
            position_near_cursor((100, 100), (140, 134), (0, 0, 1920, 1080)),
            (112, 112)
        );
    }

    #[test]
    fn popup_flips_at_bottom_right_edge() {
        assert_eq!(
            position_near_cursor((1900, 1040), (140, 134), (0, 0, 1920, 1080)),
            (1748, 894)
        );
    }

    #[test]
    fn popup_respects_negative_multi_monitor_coordinates() {
        assert_eq!(
            position_near_cursor((-100, 100), (140, 134), (-1920, 0, 1920, 1080)),
            (-240, 112)
        );
    }

    #[test]
    fn popup_clamps_when_work_area_is_smaller_than_window() {
        assert_eq!(
            position_near_cursor((9, 9), (140, 134), (10, 20, 50, 50)),
            (10, 20)
        );
    }
}
