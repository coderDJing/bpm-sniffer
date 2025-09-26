use anyhow::Result;
use tauri::{AppHandle, Emitter, Manager};
use tauri::{LogicalSize, Size, Position, PhysicalPosition};
use tauri::Url;

use crate::capture::run_capture;
use crate::lang::{is_log_zh, set_log_lang_zh};
use crate::logging::{append_log_line, emit_friendly, now_ms};
use crate::state::{AudioViz, BackendLog, DisplayBpm, CAPTURE_RUNNING, COLLECTED_LOGS, CURRENT_BPM, OUT_LEN, RESET_REQUESTED};

#[tauri::command]
pub fn start_capture(app: AppHandle) -> Result<(), String> {
    let _ = CURRENT_BPM.set(std::sync::Mutex::new(None));
    let _ = COLLECTED_LOGS.set(std::sync::Mutex::new(Vec::new()));
    let _ = RESET_REQUESTED.set(std::sync::atomic::AtomicBool::new(false));
    let flag = CAPTURE_RUNNING.get_or_init(|| std::sync::atomic::AtomicBool::new(false));
    let was_running = flag.swap(true, std::sync::atomic::Ordering::SeqCst);
    if was_running { return Ok(()); }
    std::thread::spawn(move || { let _ = run_capture(app); });
    Ok(())
}

#[tauri::command]
pub fn set_log_lang(is_zh: bool) -> Result<(), String> {
    set_log_lang_zh(is_zh);
    Ok(())
}

#[tauri::command]
pub fn get_log_lang() -> bool { is_log_zh() }

#[tauri::command]
pub fn get_current_bpm() -> Option<DisplayBpm> {
    CURRENT_BPM.get().and_then(|m| m.lock().ok().and_then(|g| *g))
}

#[tauri::command]
pub fn set_always_on_top(app: AppHandle, on_top: bool) -> Result<(), String> {
    if let Some(win) = app.get_webview_window("main") {
        win.set_always_on_top(on_top).map_err(|e| e.to_string())?;
        Ok(())
    } else { Err("window not found".into()) }
}

#[tauri::command]
pub fn enter_floating(app: AppHandle) -> Result<(), String> {
    if let Some(w) = app.get_webview_window("float") {
        let _ = w.set_always_on_top(true);
        let _ = w.set_skip_taskbar(true);
        if let Ok(Some(mon)) = w.current_monitor() {
            let size = mon.size();
            let pos = mon.position();
            let scale = mon.scale_factor();
            let wpx = (84.0 * scale) as i32;
            let margin = (40.0 * scale) as i32;
            let x = pos.x + size.width as i32 - wpx - margin;
            let y = pos.y + margin;
            let _ = w.set_position(Position::Physical(PhysicalPosition::new(x, y)));
        }
        if let (Ok(sz), Ok(sf)) = (w.inner_size(), w.scale_factor()) {
            let h_log = (sz.height as f64) / sf;
            let _ = w.set_size(Size::Logical(LogicalSize::new(h_log, h_log)));
        }
        let _ = w.navigate(Url::parse("tauri://localhost/index.html#float").unwrap_or_else(|_| Url::parse("tauri://localhost/#float").unwrap()));
        let _ = w.show();
        let _ = w.set_focus();
    }
    if let Some(main) = app.get_webview_window("main") {
        let _ = main.set_skip_taskbar(true);
        let _ = main.hide();
    }
    Ok(())
}

#[tauri::command]
pub fn exit_floating(app: AppHandle) -> Result<(), String> {
    if let Some(f) = app.get_webview_window("float") { let _ = f.hide(); }
    if let Some(main) = app.get_webview_window("main") {
        let _ = main.set_decorations(true);
        let _ = main.set_resizable(true);
        let _ = main.set_skip_taskbar(false);
        let _ = main.set_always_on_top(false);
        let _ = main.set_min_size(Some(Size::Logical(LogicalSize::new(220.0, 120.0))));
        let _ = main.set_max_size(Some(Size::Logical(LogicalSize::new(560.0, 560.0))));
        let _ = main.set_size(Size::Logical(LogicalSize::new(390.0, 390.0)));
        // 确保主窗口从隐藏状态恢复显示
        let _ = main.show();
        if let (Ok(sz), Ok(sf)) = (main.inner_size(), main.scale_factor()) {
            let w_log = (sz.width as f64) / sf;
            let h_log = (sz.height as f64) / sf;
            let _ = main.set_size(Size::Logical(LogicalSize::new(w_log + 1.0, h_log)));
            let app2 = app.clone();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(30));
                if let Some(win2) = app2.get_webview_window("main") {
                    let _ = win2.set_size(Size::Logical(LogicalSize::new(w_log, h_log)));
                }
            });
        }
        let _ = main.set_focus();
    }
    Ok(())
}

#[tauri::command]
pub fn save_float_pos(_x: i32, _y: i32) -> Result<(), String> { Ok(()) }

#[tauri::command]
pub fn get_updater_endpoints(app: AppHandle) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let conf = app.config();
    let plugins = &conf.plugins;
    if let Some(updater_cfg) = plugins.0.get("updater") {
        if let Some(arr) = updater_cfg.get("endpoints").and_then(|v: &serde_json::Value| v.as_array()) {
            for v in arr { if let Some(s) = v.as_str() { out.push(s.to_string()); } }
        }
    }
    out
}

#[tauri::command]
pub fn get_log_dir(app: AppHandle) -> Result<String, String> {
    let p = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let mut d = p.clone();
    d.push("logs");
    Ok(d.to_string_lossy().to_string())
}

#[tauri::command]
pub fn reset_backend(app: AppHandle) -> Result<(), String> {
    if let Some(flag) = RESET_REQUESTED.get() { flag.store(true, std::sync::atomic::Ordering::SeqCst); }
    let boot_txt = if is_log_zh() { "[用户] 触发后端重置" } else { "[USER] reset_backend invoked" };
    append_log_line(boot_txt);
    eprintln!("{}", boot_txt);
    let log = BackendLog { t_ms: now_ms(), msg: boot_txt.to_string() };
    let _ = app.emit_to("main", "bpm_log", log.clone());
    if let Some(cell) = COLLECTED_LOGS.get() { if let Ok(mut g) = cell.lock() { g.push(log); } }
    let _ = app.emit("viz_update", AudioViz { samples: vec![0.0; OUT_LEN], rms: 0.0 });
    if let Some(cell) = CURRENT_BPM.get() {
        if let Ok(mut guard) = cell.lock() {
            let payload = DisplayBpm { bpm: 0.0, confidence: 0.0, state: "analyzing", level: 0.0 };
            *guard = Some(payload);
            let _ = app.emit("bpm_update", payload);
        }
    }
    emit_friendly(&app, "已重置分析，正在重新聆听…", "Reset. Re-analyzing…");
    Ok(())
}

#[tauri::command]
pub fn stop_capture() -> Result<(), String> { Ok(()) }


