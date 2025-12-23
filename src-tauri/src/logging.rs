use std::fs::{self, OpenOptions};
use std::io::Write as IoWrite;
use std::sync::OnceLock;

use tauri::{AppHandle, Emitter, Manager};

use crate::lang::is_log_zh;

pub static LOG_FILE_PATH: OnceLock<std::path::PathBuf> = OnceLock::new();

// 控制台日志级别：0=静默，1=仅调性（Key）实验日志，2=全部文本日志
pub const CONSOLE_LOG_LEVEL: u8 = 1;
pub const EMIT_KEY_LOGS: bool = CONSOLE_LOG_LEVEL >= 1;
pub const EMIT_TEXT_LOGS: bool = CONSOLE_LOG_LEVEL >= 2;

pub fn append_log_line(line: &str) {
    if let Some(p) = LOG_FILE_PATH.get() {
        if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(p) {
            let _ = writeln!(f, "{}", line);
        }
    }
}

pub fn emit_friendly(app: &AppHandle, zh: impl Into<String>, en: impl Into<String>) {
    if app.get_webview_window("logs").is_some() {
        let msg = if is_log_zh() { zh.into() } else { en.into() };
        let _ = app.emit_to("logs", "friendly_log", msg);
    }
}

pub fn early_setup_logging() {
    #[cfg(target_os = "windows")]
    let base = std::env::var("APPDATA").ok().map(std::path::PathBuf::from);
    #[cfg(not(target_os = "windows"))]
    let base = dirs::home_dir();

    if let Some(mut dir) = base {
        dir.push("com.renlu.bpm-sniffer");
        dir.push("logs");
        let _ = fs::create_dir_all(&dir);
        let mut file = dir.clone();
        file.push("app.pre.log");
        if LOG_FILE_PATH.set(file.clone()).is_ok() {
            let _ = OpenOptions::new().create(true).append(true).open(&file);
            append_log_line("[BOOT-PRE] process starting before Tauri builder");
        }
        std::panic::set_hook(Box::new(move |info| {
            let ts_ms = now_ms();
            let msg = format!("[PANIC-PRE] ts={}ms {}", ts_ms, info);
            if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&file) {
                let _ = writeln!(f, "{}", msg);
            }
        }));
    }
}

pub fn setup_logging(app: &tauri::AppHandle) {
    if let Ok(mut dir) = app.path().app_data_dir() {
        dir.push("logs");
        let _ = fs::create_dir_all(&dir);
        let mut file = dir.clone();
        file.push("app.log");
        let _ = LOG_FILE_PATH.set(file.clone());
        let _ = OpenOptions::new().create(true).append(true).open(&file);
        append_log_line("[BOOT] app starting");
        std::panic::set_hook(Box::new(move |info| {
            let ts_ms = now_ms();
            let msg = format!("[PANIC] ts={}ms {}", ts_ms, info);
            if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&file) {
                let _ = writeln!(f, "{}", msg);
            }
        }));
    }
}

pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
