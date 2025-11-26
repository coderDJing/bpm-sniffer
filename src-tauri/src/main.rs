#![cfg_attr(all(not(debug_assertions), target_os = "windows"), windows_subsystem = "windows")]

#[cfg(target_os = "windows")]
mod audio;
mod bpm;
mod tempo;
mod lang;
mod state;
mod logging;
mod capture;
mod commands;

// 已迁移：logging 内部使用文件系统

use tauri::{Manager};
// use tauri::window::Color; // not needed currently
use tauri_plugin_single_instance::init as single_instance;
// use tauri_plugin_updater::UpdaterExt;
use tauri::menu::{MenuBuilder, MenuItemBuilder};
use tauri::tray::TrayIconBuilder;
use tauri::webview::WebviewWindowBuilder;
use tauri::WebviewUrl;
use tauri::{LogicalSize, Size};

use lang::{is_log_zh, set_log_lang_zh};

fn main() {
    // 超早期日志，捕捉初始化前的崩溃
    logging::early_setup_logging();
    // 初始默认：根据系统语言判定一次（Windows 读取注册表 LocaleName；其它平台回退 LANG/OSLANG）
    {
        let mut zh = false;
        #[cfg(target_os = "windows")]
        {
            use winreg::enums::*;
            use winreg::RegKey;
            // 优先 HKCU，其次 HKU/.DEFAULT，读取 LocaleName，例如 zh-CN / en-US
            let hcu = RegKey::predef(HKEY_CURRENT_USER);
            if let Ok(cp) = hcu.open_subkey("Control Panel\\International") {
                if let Ok(name) = cp.get_value::<String, _>("LocaleName") {
                    let s = name.to_lowercase(); zh = s.starts_with("zh");
                }
            }
            if !zh {
                let hku = RegKey::predef(HKEY_USERS);
                if let Ok(def) = hku.open_subkey(".DEFAULT\\Control Panel\\International") {
                    if let Ok(name) = def.get_value::<String, _>("LocaleName") {
                        let s = name.to_lowercase(); zh = s.starts_with("zh");
                    }
                }
            }
        }
        #[cfg(not(target_os = "windows"))]
        {
            if let Ok(lang) = std::env::var("LANG") { let s = lang.to_lowercase(); zh = s.starts_with("zh") || s.contains("zh_cn") || s.contains("zh-hans"); }
            if !zh { if let Ok(oslang) = std::env::var("OSLANG") { let s = oslang.to_lowercase(); zh = s.starts_with("zh"); } }
        }
        set_log_lang_zh(zh);
    }
    // 尝试加载本地环境变量文件（用于本地开发/私有更新源覆盖）
    let _ = dotenvy::from_filename(".env.local");
    let _ = dotenvy::dotenv();

    // 端点合并改为构建期脚本处理；此处仅初始化插件（端点取自 tauri.conf.json）
    let updater_builder = tauri_plugin_updater::Builder::new();

    tauri::Builder::default()
        .plugin(updater_builder.build())
        .plugin(tauri_plugin_opener::init())
        .plugin(single_instance(|app, _args, _cwd| {
            if let Some(win) = app.get_webview_window("main") { let _ = win.set_focus(); }
        }))
        .on_window_event(|window, event| {
            use tauri::WindowEvent;
            if window.label() == "main" {
                if let WindowEvent::CloseRequested { api, .. } = event {
                    // 关闭主窗口即退出程序
                    api.prevent_close();
                    let _ = window.app_handle().exit(0);
                }
            }
        })
        .setup(|app| {
            let handle = app.handle();
            logging::setup_logging(&handle);
            // 输出一次当前日志语言（用于确认）
            if is_log_zh() { logging::append_log_line("[LANG] 日志语言=中文"); eprintln!("[语言] 日志输出：中文"); } else { logging::append_log_line("[LANG] log language=EN"); eprintln!("[LANG] log language: EN"); }
            // 系统托盘与菜单（多语言）
            let logs_label = if is_log_zh() { "分析日志" } else { "Logs" };
            let about_label = if is_log_zh() { "关于" } else { "About" };
            let quit_label = if is_log_zh() { "退出" } else { "Quit" };
            let logs = MenuItemBuilder::new(logs_label).id("logs").build(app)?;
            let about = MenuItemBuilder::new(about_label).id("about").build(app)?;
            let quit = MenuItemBuilder::new(quit_label).id("quit").build(app)?;
            let menu = MenuBuilder::new(app)
                .items(&[&logs, &about, &quit])
                .build()?;

            let icon = app.default_window_icon().cloned();
            let mut tray_builder = TrayIconBuilder::new()
                .menu(&menu)
                .on_menu_event(|app, event| {
                    match event.id().as_ref() {
                        "logs" => {
                            if let Some(win) = app.get_webview_window("logs") {
                                let _ = win.set_focus();
                            } else {
                                let title = if is_log_zh() { "分析日志" } else { "Logs" };
                                let _ = WebviewWindowBuilder::new(app, "logs", WebviewUrl::App("index.html#logs".into()))
                                    .title(title)
                                    .resizable(true)
                                    .inner_size(560.0, 420.0)
                                    .build();
                            }
                        }
                        "about" => {
                            if let Some(win) = app.get_webview_window("about") {
                                let _ = win.set_focus();
                            } else {
                                // dev / prod 不同 URL
                                let _ = WebviewWindowBuilder::new(app, "about", WebviewUrl::App("index.html#about".into()))
                                    .title("About BPM Sniffer")
                                    .resizable(false)
                                    .inner_size(360.0, 360.0)
                                    .build();
                            }
                        }
                        "quit" => {
                            app.exit(0);
                        }
                        _ => {}
                    }
                });
            if let Some(ic) = icon { tray_builder = tray_builder.icon(ic); }
            
            let _ = tray_builder.build(app)?;

            // DPI 感知：使用逻辑尺寸设置窗口初始/最小尺寸，允许用户自由放大
            if let Some(win) = app.get_webview_window("main") {
                if let Ok(scale) = win.scale_factor() {
                    let _ = scale; // 仅保留查询，逻辑尺寸无需手动乘缩放
                    let base_w = 390.0f64; let base_h = 390.0f64; // 初始逻辑尺寸
                    let min_w = 220.0f64; let min_h = 120.0f64;   // 最小逻辑尺寸
                    let max_w = 560.0f64; let max_h = 560.0f64;   // 最大逻辑尺寸（限制用户拉大）
                    let _ = win.set_min_size(Some(Size::Logical(LogicalSize::new(min_w, min_h))));
                    let _ = win.set_max_size(Some(Size::Logical(LogicalSize::new(max_w, max_h))));
                    let _ = win.set_size(Size::Logical(LogicalSize::new(base_w, base_h)));
                }
            }
            if let Some(float_win) = app.get_webview_window("float") {
                let float_w = 128.0f64;
                let float_h = 128.0f64;
                let _ = float_win.set_size(Size::Logical(LogicalSize::new(float_w, float_h)));
            }

            // 开发模式下显式导航至 Vite 开发服务器，避免资源协议映射异常
            // 不再使用 dev server 导航
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::start_capture,
            commands::stop_capture,
            commands::get_current_bpm,
            commands::set_always_on_top,
            commands::get_updater_endpoints,
            commands::get_log_dir,
            commands::reset_backend,
            commands::set_log_lang,
            commands::get_log_lang,
            commands::enter_floating,
            commands::exit_floating,
            commands::save_float_pos
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
