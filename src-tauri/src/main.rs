#![cfg_attr(all(not(debug_assertions), target_os = "windows"), windows_subsystem = "windows")]

#[cfg(target_os = "windows")]
mod audio;
mod bpm;
mod tempo;
mod lang;

use std::{thread, time::Duration, sync::{Mutex, OnceLock}};
use std::sync::atomic::{AtomicBool, Ordering};
use std::fs::{self, OpenOptions};
use std::io::Write as IoWrite;
use std::collections::VecDeque;

use anyhow::Result;
use serde::Serialize;
use tauri::{AppHandle, Manager, Emitter};
use tauri_plugin_single_instance::init as single_instance;
// use tauri_plugin_updater::UpdaterExt;
use serde_json::Value as JsonValue;
use tauri::menu::{MenuBuilder, MenuItemBuilder};
use tauri::tray::TrayIconBuilder;
use tauri::Url;
use tauri::webview::WebviewWindowBuilder;
use tauri::WebviewUrl;
use tauri::{LogicalSize, Size};

use audio::AudioService;
use tempo::{make_backend, TempoBackend};
use lang::{is_log_zh, set_log_lang_zh};

#[derive(Serialize, Clone, Copy)]
struct DisplayBpm { bpm: f32, confidence: f32, state: &'static str, level: f32 }

#[derive(Serialize, Clone)]
struct BackendLog { t_ms: u64, msg: String }

// （已移除 BpmDebug 结构与相关事件收集）

#[derive(Serialize, Clone)]
struct AudioViz {
    // 下采样后的波形样本，范围约 [-1, 1]
    samples: Vec<f32>,
    // 当前包的 RMS（0-1）
    rms: f32,
}

// 已移除用于导出合并日志的 FrontendBundle 结构

static CURRENT_BPM: OnceLock<Mutex<Option<DisplayBpm>>> = OnceLock::new();
static COLLECTED_LOGS: OnceLock<Mutex<Vec<BackendLog>>> = OnceLock::new();
static RESET_REQUESTED: OnceLock<AtomicBool> = OnceLock::new();
const EMIT_TEXT_LOGS: bool = true;
// 可视化输出的下采样波形长度（与前端保持一致）
const OUT_LEN: usize = 192;
static LOG_FILE_PATH: OnceLock<std::path::PathBuf> = OnceLock::new();

// 分析支路响度标准化配置（简易 RMS 方案）
const NORM_ENABLE: bool = true;         // 标准化默认开启，仅作用于分析支路（临时关闭以便测试原始下限）
const NORM_TARGET_DBFS: f32 = -18.0;     // 目标 RMS 电平（dBFS）
const NORM_MAX_GAIN_DB: f32 = 36.0;      // 最大放大（+36 dB）
const NORM_MIN_GAIN_DB: f32 = -12.0;     // 最小衰减（-12 dB）
const NORM_SOFT_K: f32 = 1.2;            // 软限幅强度（tanh 系数，越小越温和）
// “电平过低”占位提示的判据与节流
const NORM_ATTACK: f32 = 0.25;                 // 增益上升平滑（攻）
const NORM_RELEASE: f32 = 0.08;                // 增益下降平滑（释）
// 节奏频带侧链：仅用于驱动增益计算（不改变可视化/回放）
const SC_HP_HZ: f32 = 60.0;                    // 侧链高通频率
const SC_LP_HZ: f32 = 180.0;                   // 侧链低通频率
const NORM_MAX_GAIN_DB_EXT: f32 = 42.0;        // 节奏占比良好时允许的更高最大增益
const RHYTHM_RATIO_THR: f32 = 0.25;            // 节奏频带 RMS / 全带 RMS 的阈值
const MAX_GAIN_DB_WHEN_LOW_RATIO: f32 = 18.0;  // 占比偏低时的最大增益上限

fn append_log_line(line: &str) {
    if let Some(p) = LOG_FILE_PATH.get() {
        if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(p) {
            let _ = writeln!(f, "{}", line);
        }
    }
}

// 友好日志：按事件键节流，语言可切换，输出到控制台、落盘并透传给前端
fn emit_friendly(app: &AppHandle, zh: impl Into<String>, en: impl Into<String>) {
    if app.get_webview_window("logs").is_some() {
        let msg = if is_log_zh() { zh.into() } else { en.into() };
        let _ = app.emit_to("logs", "friendly_log", msg);
    }
}

// 运行时不再刷新托盘菜单；仅在初始化时按环境判定构建

fn early_setup_logging() {
    // 在没有 AppHandle 之前，先用 APPDATA 推导日志目录
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
        // 提前设置 panic 落盘
        std::panic::set_hook(Box::new(move |info| {
            let ts_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u128)
                .unwrap_or(0);
            let msg = format!("[PANIC-PRE] ts={}ms {}", ts_ms, info);
            if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&file) {
                let _ = writeln!(f, "{}", msg);
            }
        }));
    }
}

fn setup_logging(app: &tauri::AppHandle) {
    // 放在应用数据目录下：AppData/Roaming/<identifier>/logs/app.log
    if let Ok(mut dir) = app.path().app_data_dir() {
        dir.push("logs");
        let _ = fs::create_dir_all(&dir);
        let mut file = dir.clone();
        file.push("app.log");
        let _ = LOG_FILE_PATH.set(file.clone());
        let _ = OpenOptions::new().create(true).append(true).open(&file);
        append_log_line("[BOOT] app starting");
        // panic 落盘（使用 UNIX 毫秒时间戳，避免引入依赖）
        std::panic::set_hook(Box::new(move |info| {
            let ts_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u128)
                .unwrap_or(0);
            let msg = format!("[PANIC] ts={}ms {}", ts_ms, info);
            if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&file) {
                let _ = writeln!(f, "{}", msg);
            }
        }));
    }
}

#[tauri::command]
fn start_capture(app: AppHandle) -> Result<(), String> {
    let _ = CURRENT_BPM.set(Mutex::new(None));
    let _ = COLLECTED_LOGS.set(Mutex::new(Vec::new()));
    let _ = RESET_REQUESTED.set(AtomicBool::new(false));
    thread::spawn(move || {
        if let Err(_e) = run_capture(app) { }
    });
    Ok(())
}
#[tauri::command]
fn set_log_lang(is_zh: bool) -> Result<(), String> {
    set_log_lang_zh(is_zh);
    Ok(())
}

#[tauri::command]
fn get_log_lang() -> bool {
    is_log_zh()
}


#[tauri::command]
fn get_current_bpm() -> Option<DisplayBpm> {
    CURRENT_BPM.get().and_then(|m| m.lock().ok().and_then(|g| *g))
}
// 已移除导出 JSON 相关命令

#[tauri::command]
fn set_always_on_top(app: AppHandle, on_top: bool) -> Result<(), String> {
    if let Some(win) = app.get_webview_window("main") {
        win.set_always_on_top(on_top).map_err(|e| e.to_string())?;
        Ok(())
    } else {
        Err("window not found".into())
    }
}

#[tauri::command]
fn get_updater_endpoints(app: AppHandle) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let conf = app.config();
    let plugins = &conf.plugins;
    if let Some(updater_cfg) = plugins.0.get("updater") {
        // 期望结构：{"endpoints": ["url1", "url2", ...]}
        if let Some(arr) = updater_cfg.get("endpoints").and_then(|v: &JsonValue| v.as_array()) {
            for v in arr {
                if let Some(s) = v.as_str() {
                    out.push(s.to_string());
                }
            }
        }
    }
    out
}

#[tauri::command]
fn get_log_dir(app: AppHandle) -> Result<String, String> {
    let p = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let mut d = p.clone();
    d.push("logs");
    Ok(d.to_string_lossy().to_string())
}

#[tauri::command]
fn reset_backend(app: AppHandle) -> Result<(), String> {
    if let Some(flag) = RESET_REQUESTED.get() {
        flag.store(true, Ordering::SeqCst);
    }
    // 日志：记录刷新请求
    let boot_txt = if is_log_zh() { "[用户] 触发后端重置" } else { "[USER] reset_backend invoked" };
    append_log_line(boot_txt);
    eprintln!("{}", boot_txt);
    let log = BackendLog { t_ms: now_ms(), msg: boot_txt.to_string() };
    let _ = app.emit_to("main", "bpm_log", log.clone());
    if let Some(cell) = COLLECTED_LOGS.get() { if let Ok(mut g) = cell.lock() { g.push(log); } }
    // 立即向前端发一次清零，提升“已重置”的即时反馈
    let _ = app.emit_to("main", "viz_update", AudioViz { samples: vec![0.0; OUT_LEN], rms: 0.0 });
    if let Some(cell) = CURRENT_BPM.get() {
        if let Ok(mut guard) = cell.lock() {
            let payload = DisplayBpm { bpm: 0.0, confidence: 0.0, state: "analyzing", level: 0.0 };
            *guard = Some(payload);
            let _ = app.emit_to("main", "bpm_update", payload);
        }
    }
    // 友好提示
    emit_friendly(&app, "已重置分析，正在重新聆听…", "Reset. Re-analyzing…");
    Ok(())
}

// 不再做 0.5 步进四舍五入，交由前端格式化显示

fn level_from_frames(frames: &[f32]) -> f32 {
    if frames.is_empty() { return 0.0; }
    let mut sum = 0.0f32;
    for &s in frames { sum += s * s; }
    let rms = (sum / frames.len() as f32).sqrt();
    let db = 20.0 * (rms.max(1e-9)).log10();
    ((db + 60.0) / 60.0).clamp(0.0, 1.0)
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn run_capture(app: AppHandle) -> Result<()> {
    let (svc, rx, sr_rx) = AudioService::start_loopback()?;
    // 友好提示：开始捕获系统音频（仅在日志窗口存在时发送）
    emit_friendly(&app, "已开始捕获系统音频", "Started capturing system audio");

    let mut backend: Box<dyn TempoBackend> = make_backend(svc.sample_rate());

    let hi_th = 0.40f32; // 更快进入 tracking
    let lo_th = 0.25f32;
    let mut hi_cnt = 0usize;
    let mut lo_cnt = 0usize;
    let mut tracking = false;

    let mut ever_locked = false;
    let mut none_cnt = 0usize;

    // 滑动窗口：窗口 2s，步长 0.5s（重叠 75%）
    let mut sr_usize = svc.sample_rate() as usize;
    let mut target_len = sr_usize * 2;
    let mut hop_len = sr_usize / 2;
    let mut window: VecDeque<f32> = VecDeque::with_capacity(target_len * 2);
    let mut no_data_ms: u64 = 0;
    let mut silent_win_cnt: usize = 0;
    let mut anchor_bpm: Option<f32> = None; // 高置信度时的锚点，用于半/倍频纠偏
    // 显示平滑缓存（稳定优先：时间窗口中值 + EMA）
    let mut disp_hist: VecDeque<f32> = VecDeque::with_capacity(7);
    let mut ema_disp: Option<f32> = None;
    // 稳定聚合窗口（毫秒）：扩大到 1.5s 以用于软门稳定性判据（MAD）
    let stable_win_ms: u64 = 1500;
    let mut stable_vals: VecDeque<(f32, u64)> = VecDeque::with_capacity(256);

    // 可视化事件节流：至少间隔 33ms 发送一次（~30fps）
    let mut last_viz_ms: u64 = 0;
    // 切歌快速重锁相关：截止时间与触发器统计
    let mut fast_relock_deadline: Option<u64> = None;
    let mut prev_rms_db: Option<f32> = None;
    let mut recent_none_flag: bool = false;
    let mut dev_from_lock_cnt: u8 = 0;
    // 记录是否处于静音，用于检测“恢复有声”
    let mut was_silent_flag: bool = false;
    // 记录上一次原始估计来自短窗/长窗
    let mut last_from_short: Option<bool> = None;
    // 记录最近一次“非灰显（高亮）”显示的整数与状态，用于软门同整数时维持高亮
    let mut last_hard_int: Option<i32> = None;
    let mut last_hard_state: Option<&'static str> = None;
    // 简易噪声底估计（RMS）与显示层 alpha-beta 预测器
    let mut noise_floor_rms: f32 = 0.01;
    let mut trk_x: Option<f32> = None;
    let mut trk_v: f32 = 0.0;
    // 近期整数直方统计，用于切歌时的主导整数快速采纳
    let mut recent_ints: VecDeque<(i32, u64)> = VecDeque::with_capacity(16);
    // 候选整数（未必已显示）的直方统计，用于检测被卡住时的主导切换
    let mut recent_ints_cand: VecDeque<(i32, u64)> = VecDeque::with_capacity(32);
    // 请求在下一次整数锁阶段清空锁
    let mut force_clear_lock: bool = false;
    // 标准化运行时状态
    let mut norm_gain_db_smooth: f32 = 0.0; // 平滑后的 dB 增益
    // 已移除低电平提示：不再跟踪连续低电平
    // 侧链滤波状态（简单一阶高通+低通）
    let mut sc_hp_alpha: f32 = 0.0;
    let mut sc_lp_alpha: f32 = 0.0;
    let mut sc_hp_lp_prev: f32 = 0.0;
    let mut sc_lp_prev: f32 = 0.0;

    loop {
        // 软重置：收到请求后清空内部状态与窗口，向前端清零
        if let Some(flag) = RESET_REQUESTED.get() {
            if flag.swap(false, Ordering::SeqCst) {
                window.clear();
                disp_hist.clear(); ema_disp = None; prev_rms_db = None; anchor_bpm = None; stable_vals.clear();
                hi_cnt = 0; lo_cnt = 0; tracking = false; ever_locked = false; none_cnt = 0; dev_from_lock_cnt = 0; force_clear_lock = true;
                recent_none_flag = false; last_hard_int = None; last_hard_state = None; trk_x = None; trk_v = 0.0;
                recent_ints.clear(); recent_ints_cand.clear(); noise_floor_rms = 0.01;
                let _ = app.emit_to("main", "viz_update", AudioViz { samples: vec![0.0; OUT_LEN], rms: 0.0 });
                if let Some(cell) = CURRENT_BPM.get() { if let Ok(mut guard) = cell.lock() { let payload = DisplayBpm { bpm: 0.0, confidence: 0.0, state: "analyzing", level: 0.0 }; *guard = Some(payload); let _ = app.emit_to("main", "bpm_update", payload); } }
            }
        }
        // 仅 Simple；阻塞接收，直至累积 ≥ 窗口大小（带超时）
        while window.len() < target_len {
            // 优先监听采样率变化；若变化则重建后端并清空窗口
            if let Ok(new_sr) = sr_rx.try_recv() {
                if new_sr as usize != sr_usize {
                    sr_usize = new_sr as usize;
                    target_len = sr_usize * 2;
                    hop_len = sr_usize / 2;
                    backend = make_backend(new_sr);
                    // 更新侧链滤波系数
                    let fs = new_sr as f32;
                    sc_hp_alpha = (-2.0 * std::f32::consts::PI * SC_HP_HZ / fs).exp();
                    sc_lp_alpha = (-2.0 * std::f32::consts::PI * SC_LP_HZ / fs).exp();
                    sc_hp_lp_prev = 0.0; sc_lp_prev = 0.0;
                    window.clear();
                    disp_hist.clear(); ema_disp = None; prev_rms_db = None; anchor_bpm = None; stable_vals.clear();
                    hi_cnt = 0; lo_cnt = 0; tracking = false; ever_locked = false; none_cnt = 0; dev_from_lock_cnt = 0; force_clear_lock = true;
                    // 立即向前端发一次可视化清零，避免残影
                    let _ = app.emit_to("main", "viz_update", AudioViz { samples: vec![0.0; OUT_LEN], rms: 0.0 });
                    // 告知 0 BPM 状态，直到新数据到来
                    if let Some(cell) = CURRENT_BPM.get() { if let Ok(mut guard) = cell.lock() { let payload = DisplayBpm { bpm: 0.0, confidence: 0.0, state: "analyzing", level: 0.0 }; *guard = Some(payload); let _ = app.emit_to("main", "bpm_update", payload); } }
                }
            }
            match rx.recv_timeout(Duration::from_millis(20)) {
                Ok(mut buf) => {
                    // 先基于当前包生成可视化，再将其推入窗口，避免 drain 后访问空缓冲
                    if !buf.is_empty() {
                        let len = buf.len();
                        let mut rms_acc = 0.0f32;
                        for &v in &buf { rms_acc += v * v; }
                        let rms = (rms_acc / len as f32).sqrt().min(1.0);
                        // 可视化 RMS：对极低电平直接视为静音，立即归零
                        let silent_cut = 0.015f32;
                        let viz_rms = if rms < silent_cut { 0.0 } else { rms };
                        let nowv = now_ms();
                        if nowv.saturating_sub(last_viz_ms) >= 33 {
                            if viz_rms == 0.0 {
                                // 极低电平：波形与 RMS 同步归零
                                let _ = app.emit_to("main", "viz_update", AudioViz { samples: vec![0.0; OUT_LEN], rms: 0.0 });
                            } else {
                                // 下采样生成可视化波形
                                let step = (len as f32 / OUT_LEN as f32).max(1.0);
                                let mut out: Vec<f32> = Vec::with_capacity(OUT_LEN);
                                let mut idx_f = 0.0f32;
                                for _ in 0..OUT_LEN {
                                    let i0 = idx_f as usize;
                                    let i1 = ((idx_f + step) as usize).min(len);
                                    let mut acc = 0.0f32;
                                    let mut cnt = 0usize;
                                    if i0 < i1 {
                                        // i1 不包含，因此无需 -1，索引范围总小于 len
                                        for i in i0..i1 { acc += buf[i]; cnt += 1; }
                                    }
                                    out.push(if cnt > 0 { (acc / cnt as f32).clamp(-1.0, 1.0) } else { 0.0 });
                                    idx_f += step;
                                }
                                let _ = app.emit_to("main", "viz_update", AudioViz { samples: out, rms: viz_rms });
                            }
                            last_viz_ms = nowv;
                        }
                    } else {
                        let nowv = now_ms();
                        if nowv.saturating_sub(last_viz_ms) >= 33 {
                            let _ = app.emit_to("main", "viz_update", AudioViz { samples: vec![0.0; OUT_LEN], rms: 0.0 });
                            last_viz_ms = nowv;
                        }
                    }

                    // 再推入分析窗口
                    for s in buf.drain(..) { window.push_back(s); }
                    no_data_ms = 0;
                }
                Err(_) => {
                    no_data_ms += 20;
                    // 持续无数据时，仍以 ~30fps 推送零波形与零RMS，避免前端残留上一帧
                    let nowv = now_ms();
                    if nowv.saturating_sub(last_viz_ms) >= 33 {
                        let _ = app.emit_to("main", "viz_update", AudioViz { samples: vec![0.0; OUT_LEN], rms: 0.0 });
                        last_viz_ms = nowv;
                    }
                    if no_data_ms >= 1500 {
                        // 长时间无数据，视为静音，推送 0 BPM
                        tracking = false; ever_locked = false; hi_cnt = 0; lo_cnt = 0;
                        if let Some(cell) = CURRENT_BPM.get() {
                            if let Ok(mut guard) = cell.lock() {
                                let payload = DisplayBpm { bpm: 0.0, confidence: 0.0, state: "analyzing", level: 0.0 };
                                *guard = Some(payload);
                                let _ = app.emit_to("main", "bpm_update", payload);
                            }
                        }
                        no_data_ms = 0;
                    }
                }
            }
        }
        // 取窗口前 2s 作为当前帧（分析原始帧）
        let mut frames: Vec<f32> = Vec::with_capacity(target_len);
        for i in 0..target_len { if let Some(&v) = window.get(i) { frames.push(v); } }
        // 初始化侧链滤波系数（首次）
        if sc_hp_alpha == 0.0 || sc_lp_alpha == 0.0 {
            let fs = svc.sample_rate() as f32;
            sc_hp_alpha = (-2.0 * std::f32::consts::PI * SC_HP_HZ / fs).exp();
            sc_lp_alpha = (-2.0 * std::f32::consts::PI * SC_LP_HZ / fs).exp();
            sc_hp_lp_prev = 0.0; sc_lp_prev = 0.0;
        }

        let level = level_from_frames(&frames);
        // 计算当前帧能量 dB（用于能量跳变检测）
        let mut sumsq = 0.0f32;
        for &s in &frames { sumsq += s * s; }
        let rms = (sumsq / frames.len() as f32).sqrt();
        let cur_db = 20.0 * (rms.max(1e-9)).log10();
        // 更新噪声底（EMA，偏保守，响应慢一点）
        noise_floor_rms = noise_floor_rms * 0.99 + rms * 0.01;
        let is_silent = level < 0.03; // 略提高阈值，加速静音判定

        if is_silent {
            // 无声：直接显示 0 BPM，清空锁定状态
            tracking = false; ever_locked = false; hi_cnt = 0; lo_cnt = 0;
            // 归零标准化状态
            norm_gain_db_smooth = 0.0;
            silent_win_cnt = silent_win_cnt.saturating_add(1);
            if let Some(cell) = CURRENT_BPM.get() {
                if let Ok(mut guard) = cell.lock() {
                    let payload = DisplayBpm { bpm: 0.0, confidence: 0.0, state: "analyzing", level };
                    *guard = Some(payload);
                    let _ = app.emit_to("main", "bpm_update", payload);
                }
            }
            // （已移除 bpm_debug 事件收集）
            disp_hist.clear(); ema_disp = None; prev_rms_db = None;
            // 友好提示：环境安静
            emit_friendly(&app, "检测到环境安静，BPM 为 0（等待声音）", "Silence detected. BPM is 0 (waiting for audio)");
            was_silent_flag = true;
            // 滑动步进
            for _ in 0..hop_len { let _ = window.pop_front(); }
            continue;
        }
        // 若从静音恢复到有声
        if was_silent_flag {
            emit_friendly(&app, "检测到声音，开始分析…", "Audio detected. Analyzing…");
            was_silent_flag = false;
        }
        silent_win_cnt = 0;

        // 分析支路：在送入后端前做 RMS 标准化与软限幅（不影响可视化）
        let mut frames_for_analysis: Vec<f32> = frames.clone();
        if NORM_ENABLE {
            // 估计帧 RMS 与目标增益
            let mut sumsq = 0.0f32; for &s in &frames_for_analysis { sumsq += s * s; }
            let rms = (sumsq / frames_for_analysis.len() as f32).sqrt().max(1e-9);
            // 节奏频带侧链 RMS
            let mut sc_sumsq = 0.0f32;
            for &s in &frames_for_analysis {
                let hp_lp = sc_hp_alpha * sc_hp_lp_prev + (1.0 - sc_hp_alpha) * s;
                let hp = s - hp_lp; sc_hp_lp_prev = hp_lp;
                let lp = sc_lp_alpha * sc_lp_prev + (1.0 - sc_lp_alpha) * hp; sc_lp_prev = lp;
                sc_sumsq += lp * lp;
            }
            let sc_rms = (sc_sumsq / frames_for_analysis.len() as f32).sqrt().max(1e-9);
            let rhythm_ratio = (sc_rms / rms.max(1e-9)).clamp(0.0, 1.0);
            let cur_dbfs = 20.0 * rms.log10();
            let mut need_gain_db = NORM_TARGET_DBFS - cur_dbfs;
            // 动态最大增益：节奏占比较高允许更大增益
            let dyn_max_gain = if rhythm_ratio >= RHYTHM_RATIO_THR { NORM_MAX_GAIN_DB.max(NORM_MAX_GAIN_DB_EXT) } else { NORM_MAX_GAIN_DB.min(MAX_GAIN_DB_WHEN_LOW_RATIO) };
            if need_gain_db > dyn_max_gain { need_gain_db = dyn_max_gain; }
            if need_gain_db < NORM_MIN_GAIN_DB { need_gain_db = NORM_MIN_GAIN_DB; }
            // 平滑（不同攻/释）
            let a = if need_gain_db > norm_gain_db_smooth { NORM_ATTACK } else { NORM_RELEASE };
            norm_gain_db_smooth = norm_gain_db_smooth * (1.0 - a) + need_gain_db * a;
            let lin_gain = 10f32.powf(norm_gain_db_smooth / 20.0);
            // 应用增益
            if lin_gain != 1.0 {
                for x in &mut frames_for_analysis { *x *= lin_gain; }
            }
            // 软限幅（tanh），温和保护峰值
            if NORM_SOFT_K > 0.0 {
                for x in &mut frames_for_analysis { let y = (NORM_SOFT_K * *x).tanh(); *x = y / NORM_SOFT_K; }
            }
            // 低电平提示逻辑已删除
        }

        if let Some(raw) = backend.process(&frames_for_analysis) {
            none_cnt = 0;
            // 短窗/长窗切换提示（来源变化）
            if last_from_short.map_or(true, |v| v != raw.from_short) {
                if raw.from_short { emit_friendly(&app, "切换为短窗优先（更快跟随变化）", "Switched to short-window (faster response)"); }
                else { emit_friendly(&app, "切换为长窗优先（更稳更准）", "Switched to long-window (more stable)"); }
                last_from_short = Some(raw.from_short);
            }
            // 能量跳变触发切歌快速重锁
            if let Some(pdb) = prev_rms_db {
                if (cur_db - pdb).abs() >= 6.0 {
                    fast_relock_deadline = Some(now_ms().saturating_add(2000));
                    anchor_bpm = None; stable_vals.clear();
                    emit_friendly(&app, "检测到变化，快速锁定中…", "Change detected. Fast relock…");
                }
            }
            prev_rms_db = Some(cur_db);

            let mut conf = raw.confidence.min(0.9);
            conf = conf.powf(0.9);
            // 节拍SNR加权：当鼓点相对噪声底更干净时，上调有效置信度
            let snr = (rms / noise_floor_rms.max(1e-6)).max(0.0);
            let snr_boost = (snr / 2.5).clamp(0.6, 1.15); // SNR≈2.5 时1.0；弱时0.6，强时上限1.15
            conf = (conf * snr_boost).clamp(0.0, 0.95);

            // 对超短窗（win_sec 很小）采用更宽松的追踪阈值，避免长期停留在 analyzing
            let is_ultra_short = raw.win_sec <= 0.1;
            let (thr_hi, thr_lo) = if is_ultra_short { (0.15f32, 0.08f32) } else { (hi_th, lo_th) };

            if conf >= thr_hi { hi_cnt += 1; } else { hi_cnt = 0; }
            if conf <= thr_lo { lo_cnt += 1; } else { lo_cnt = 0; }

            if !tracking && hi_cnt >= 3 {
                tracking = true; ever_locked = true;
                if EMIT_TEXT_LOGS {
                    let txt = if is_log_zh() { format!("[状态] 进入追踪 bpm={:.1} 置信度={:.2}", raw.bpm, conf) } else { format!("[STATE] tracking=ON  bpm={:.1} conf={:.2}", raw.bpm, conf) };
                    eprintln!("{}", txt);
                    let log = BackendLog { t_ms: now_ms(), msg: txt.clone() };
                    let _ = app.emit_to("main", "bpm_log", log.clone());
                    if let Some(cell) = COLLECTED_LOGS.get() { if let Ok(mut g) = cell.lock() { g.push(log); } }
                }
                // 友好提示：锁定节拍
                emit_friendly(&app, format!("已锁定节拍：约 {:.0} BPM（稳定度 {:.0}%）", raw.bpm, conf*100.0), format!("Beat locked: ~{:.0} BPM (confidence {:.0}%)", raw.bpm, conf*100.0));
                // 友好提示：节拍已稳定，开始高亮显示
                emit_friendly(&app, "节拍已稳定，开始高亮显示", "Beat stable. Highlighting");
            }
            if tracking && lo_cnt >= 2 {
                tracking = false;
                if EMIT_TEXT_LOGS {
                    let txt = if is_log_zh() { format!("[状态] 退出追踪 置信度={:.2}", conf) } else { format!("[STATE] tracking=OFF conf={:.2}", conf) };
                    eprintln!("{}", txt);
                    let log = BackendLog { t_ms: now_ms(), msg: txt.clone() };
                    let _ = app.emit_to("main", "bpm_log", log.clone());
                    if let Some(cell) = COLLECTED_LOGS.get() { if let Ok(mut g) = cell.lock() { g.push(log); } }
                }
                // 友好提示：丢失节拍
                emit_friendly(&app, "节拍暂不稳定，正在重新分析…", "Beat unstable. Re-analyzing…");
            }

            let state: &str = if tracking { "tracking" } else if ever_locked { "uncertain" } else { "analyzing" };
            let in_fast = fast_relock_deadline.map_or(false, |t| now_ms() < t);

            // 锚点纠偏候选：{raw, 1/2x, 2x, 2/3x, 3/2x}
            let mut disp = raw.bpm;
            let mut _corr_kind: &'static str = "raw";
            if let Some(base) = anchor_bpm {
                let mut best_bpm = disp;
                let mut best_err = (disp - base).abs();
                let mut best_kind: &'static str = "raw";

                let try_cand = |val: f32, kind: &'static str, best_bpm: &mut f32, best_err: &mut f32, best_kind: &mut &'static str| {
                    if val >= 60.0 && val <= 200.0 {
                        let err = (val - base).abs();
                        // 只要明显更接近锚点就采用（阈值略放宽）
                        if err + 0.2 < *best_err { *best_err = err; *best_bpm = val; *best_kind = kind; }
                    }
                };

                // 候选集合（含 2/3 与 3/2，优先靠近锚点）
                try_cand(disp * 0.5, "half", &mut best_bpm, &mut best_err, &mut best_kind);
                try_cand(disp * 2.0, "dbl", &mut best_bpm, &mut best_err, &mut best_kind);
                try_cand(disp * (2.0/3.0), "two_thirds", &mut best_bpm, &mut best_err, &mut best_kind);
                try_cand(disp * (3.0/2.0), "three_halves", &mut best_bpm, &mut best_err, &mut best_kind);

                if best_bpm != disp && EMIT_TEXT_LOGS { if is_log_zh() { eprintln!("[谐波校正] {} -> {:.1} (基准={:.1}, 原始={:.1})", best_kind, best_bpm, base, disp); } else { eprintln!("[CORR] {} -> {:.1} (base={:.1}, raw={:.1})", best_kind, best_bpm, base, disp); } }
                if best_bpm != disp { emit_friendly(&app, format!("已纠正谐波：{} → {:.1}（参考 {:.1}，原始 {:.1}）", best_kind, best_bpm, base, disp), format!("Harmonic correction: {} → {:.1} (ref {:.1}, raw {:.1})", best_kind, best_bpm, base, disp)); }
                disp = best_bpm; _corr_kind = best_kind;
            }

            // EDM 范围标准化：将候选按 2x/0.5x 折算到 [91, 180]
            if disp < 91.0 || disp > 180.0 {
                let mut t = disp;
                for _ in 0..4 {
                    if t < 91.0 { t *= 2.0; }
                    else if t > 180.0 { t *= 0.5; }
                    else { break; }
                }
                if t >= 91.0 && t <= 180.0 { disp = t; _corr_kind = "edm_norm"; }
            }

            // 显示层整数锁定（带滞后）：不改变内部状态，仅用于显示
            // 满足：conf≥0.80 且最近多次都落在同一整数±1内
            {
                static mut LOCK_INT: Option<i32> = None;
                static mut LOCK_CNT: u8 = 0;
                static mut UNLOCK_CNT: u8 = 0;
                static mut LAST_SHOW_MS: Option<u64> = None; // 最近一次成功显示的时间，用于TTL
                // 候选整数切换计数（用于避免被错误锁在 128）
                static mut ALT_INT: Option<i32> = None;
                static mut ALT_CNT: u8 = 0;
                // 基于 TTL 的锁清理：10s 没有成功显示则清空锁，避免切歌残留
                unsafe {
                    if let Some(last) = LAST_SHOW_MS { if now_ms().saturating_sub(last) > 10_000 { LOCK_INT = None; LOCK_CNT = 0; UNLOCK_CNT = 0; } }
                }
                let disp_round = disp.round() as i32;
                let diff = (disp - disp_round as f32).abs();
                let within = diff <= 0.6; // 进一步收紧吸附半径，抑制 130.5 和误吸到 131
                unsafe {
                    if in_fast || force_clear_lock { LOCK_INT = None; LOCK_CNT = 0; UNLOCK_CNT = 0; ALT_INT = None; ALT_CNT = 0; force_clear_lock = false; }
                    // 大偏差且高置信度：立即解锁，避免被旧整数粘住（切歌等场景）
                    if let Some(n) = LOCK_INT { if conf >= 0.85 && (disp - n as f32).abs() >= 2.0 { LOCK_INT = None; LOCK_CNT = 0; UNLOCK_CNT = 0; } }
                    if conf >= 0.80 && within {
                        if let Some(n) = LOCK_INT { if n == disp_round { LOCK_CNT = LOCK_CNT.saturating_add(1); } else { LOCK_CNT = 1; LOCK_INT = Some(disp_round); } }
                        else { LOCK_INT = Some(disp_round); LOCK_CNT = 1; }
                        // 高置信度时，立即将计数提升至阈值，首帧也能吸附
                        if conf >= 0.90 && diff <= 0.4 { if LOCK_CNT < 2 { LOCK_CNT = 2; } }
                        if LOCK_CNT >= 2 { UNLOCK_CNT = 0; disp = disp_round as f32; }
                    } else if let Some(n) = LOCK_INT {
                        // 更稳的解锁：高置信度且持续偏离才解锁
                        if conf >= 0.82 && (disp - n as f32).abs() > 1.3 {
                            UNLOCK_CNT = UNLOCK_CNT.saturating_add(1);
                            if UNLOCK_CNT >= 3 { LOCK_INT = None; LOCK_CNT = 0; UNLOCK_CNT = 0; }
                        } else {
                            UNLOCK_CNT = 0;
                        }
                        // 相邻整数快速切换：连续观察到另一个整数且足够靠近时，直接切换锁
                        let switch_conf = if in_fast { 0.70 } else { 0.82 };
                        let switch_need = if in_fast { 2 } else { 3 };
                        if conf >= switch_conf {
                            let near_other = (disp - disp_round as f32).abs() <= 0.4 && disp_round != n;
                            if near_other {
                                if ALT_INT == Some(disp_round) { ALT_CNT = ALT_CNT.saturating_add(1); } else { ALT_INT = Some(disp_round); ALT_CNT = 1; }
                                if ALT_CNT as i32 >= switch_need {
                                    LOCK_INT = Some(disp_round);
                                    LOCK_CNT = 2; // 视为已稳定
                                    UNLOCK_CNT = 0;
                                    disp = disp_round as f32;
                                }
                            } else {
                                ALT_CNT = 0;
                            }
                        } else {
                            ALT_CNT = 0;
                        }
                        // 锁偏离统计：偏离 ≥ 8 BPM 计数，用于触发快速重锁
                        if (disp - n as f32).abs() >= 8.0 { dev_from_lock_cnt = dev_from_lock_cnt.saturating_add(1); } else { dev_from_lock_cnt = 0; }
                    }
                }
                // 若本帧满足显示门槛，则更新 TTL 时间戳（用于下一帧的TTL清理）
                unsafe { if conf >= 0.80 { LAST_SHOW_MS = Some(now_ms()); } }
            }

            // 记录候选整数序列（已做EDM标准化后），用于无需“切歌检测”的多数派强制切换
            {
                let nowh = now_ms();
                recent_ints_cand.push_back((disp.round() as i32, nowh));
                while let Some(&(_, t0)) = recent_ints_cand.front() { if nowh.saturating_sub(t0) > 1500 { recent_ints_cand.pop_front(); } else { break; } }
            }

            // 最近 none 恢复触发：下一帧进入快速重锁期（若达到基础置信度）
            if recent_none_flag && conf >= 0.50 {
                fast_relock_deadline = Some(now_ms().saturating_add(2000));
                anchor_bpm = None; recent_none_flag = false; stable_vals.clear();
                emit_friendly(&app, "从空段恢复，快速锁定中…", "Recovered from none, fast relock…");
            }
            if dev_from_lock_cnt >= 2 {
                fast_relock_deadline = Some(now_ms().saturating_add(2000));
                anchor_bpm = None; dev_from_lock_cnt = 0; stable_vals.clear();
                emit_friendly(&app, "检测到与锁定值偏离，快速锁定中…", "Deviation from locked value, fast relock…");
                // 若近期整数直方统计中出现新主导整数（计数≥3），请求清空锁以便快速吸附
                let nowh = now_ms();
                let mut counts: std::collections::HashMap<i32, usize> = std::collections::HashMap::new();
                for (v, t) in recent_ints.iter().rev() { if nowh.saturating_sub(*t) <= 1500 { *counts.entry(*v).or_insert(0) += 1; } else { break; } }
                let mut best: Option<(i32, usize)> = None;
                for (k, c) in counts { if best.map_or(true, |(_, bc)| c > bc) { best = Some((k, c)); } }
                if let Some((_, c)) = best { if c >= 3 { force_clear_lock = true; } }
            }

            // 稳定聚合：600ms 窗口中值 + 轻微 EMA(0.85/0.15)
            let now_t = now_ms();
            stable_vals.push_back((disp, now_t));
            while let Some(&(_, t0)) = stable_vals.front() { if now_t.saturating_sub(t0) > stable_win_ms { stable_vals.pop_front(); } else { break; } }
            let mut win_sorted: Vec<f32> = stable_vals.iter().map(|(v,_)| *v).collect();
            if win_sorted.is_empty() { win_sorted.push(disp); }
            win_sorted.sort_by(|a,b| a.partial_cmp(b).unwrap());
            let mid = win_sorted[win_sorted.len()/2];
            let smoothed = if let Some(prev) = ema_disp { prev * 0.85 + mid * 0.15 } else { mid };
            ema_disp = Some(smoothed);

            // 轻量 alpha-beta 预测器：在低置信度时以预测为主，提升过渡期跟随性
            let alpha = 0.28f32; // 位置增益
            let beta  = 0.06f32; // 速度增益
            let dt    = 0.5f32;  // 以 hop_len≈0.5s 为预测步长
            if trk_x.is_none() { trk_x = Some(smoothed); trk_v = 0.0; }
            if let Some(mut x) = trk_x {
                // 预测
                let x_pred = x + trk_v * dt;
                // 观测
                let z = smoothed;
                // 低置信度时加大预测权重（减小增益）
                let gain_scale = if conf < 0.70 { 0.6 } else if conf < 0.80 { 0.8 } else { 1.0 };
                let a = alpha * gain_scale;
                let b = beta  * gain_scale;
                let r = z - x_pred;
                x = x_pred + a * r;
                trk_v = trk_v + (b * r) / dt;
                trk_x = Some(x);
                // 将预测器输出反哺为最终显示平滑的候选（仅在软门或低置信时生效）
                if conf < 0.80 { ema_disp = Some(x); }
            }

            if let Some(cell) = CURRENT_BPM.get() {
                if let Ok(mut guard) = cell.lock() {
                    // 硬门：置信度达到 0.80，且通过离群抑制
                    let mut allow_hard = conf >= 0.80;
                    // 如无锚点，使用“当前已持有的上一帧显示值整数”作为临时基准，避免二次加锁造成死锁
                    let mut base_for_guard: Option<f32> = anchor_bpm;
                    if base_for_guard.is_none() {
                        if let Some(prev) = *guard {
                            if prev.bpm > 0.0 { base_for_guard = Some(prev.bpm.round()); }
                        }
                    }
                    if allow_hard {
                        if let Some(base) = base_for_guard { if tracking {
                            let rel = (disp - base).abs() / base.max(1e-6);
                            if rel > 0.12 {
                                // 再试图通过谐波回拉
                                let mut best = disp;
                                let mut err = rel;
                                let try_back = |v: f32, best: &mut f32, err: &mut f32| { if v>=60.0 && v<=200.0 { let e = (v-base).abs()/base.max(1e-6); if e < *err { *err = e; *best = v; } } };
                                try_back(disp*0.5, &mut best, &mut err);
                                try_back(disp*2.0, &mut best, &mut err);
                                try_back(disp*(2.0/3.0), &mut best, &mut err);
                                try_back(disp*(3.0/2.0), &mut best, &mut err);
                                if err <= 0.08 { disp = best; } else { allow_hard = false; if EMIT_TEXT_LOGS { if is_log_zh() { eprintln!("[离群] 抑制显示 bpm={:.1} 基准={:.1} 相对误差={:.3}", disp, base, rel); } else { eprintln!("[OUTLIER] suppress show bpm={:.1} base={:.1} rel={:.3}", disp, base, rel); } } emit_friendly(&app, format!("忽略异常候选：{:.1} BPM（偏离 {:.0}%）", disp, rel*100.0), format!("Ignored outlier: {:.1} BPM (deviation {:.0}%)", disp, rel*100.0)); }
                            }
                        }}
                        // 范围限幅：若候选超出 91–180 且存在基准，则直接抑制
                        if allow_hard {
                            if let Some(base) = base_for_guard {
                                if (disp > 180.0 || disp < 91.0) && conf >= 0.80 {
                                    allow_hard = false;
                                    if EMIT_TEXT_LOGS { if is_log_zh() { eprintln!("[离群] 超出范围 bpm={:.1} 基准={:.1}", disp, base); } else { eprintln!("[OUTLIER] suppress out-of-range bpm={:.1} base={:.1}", disp, base); } }
                                    emit_friendly(&app, format!("放弃越界结果：{:.1} BPM（当前范围 91–180）", disp), format!("Dropped out-of-range result: {:.1} BPM (range 91–180)", disp));
                                }
                            }
                        }
                    }
                    // 软门：置信度较低但稳定（MAD小），在快速期软门阈值更低
                    let soft_thr = if in_fast { 0.50 } else { 0.55 };
                    let allow_soft = if !allow_hard && conf >= soft_thr {
                        // 使用“近邻一致性”而非对全局中位数的MAD，以避免旧歌值拖拽
                        let now_t2 = now_ms();
                        let recent_span_ms = if in_fast { 1000 } else { 1500 };
                        let recent: Vec<f32> = stable_vals.iter()
                            .rev()
                            .take_while(|(_, t)| now_t2.saturating_sub(*t) <= recent_span_ms)
                            .map(|(v,_)| *v)
                            .collect();
                        let n = recent.len();
                        if n >= 2 {
                            let near = recent.into_iter().filter(|v| (*v - disp).abs() <= 0.8).count();
                            let need = if in_fast { 2 } else { 3 };
                            near >= need && disp >= 60.0 && disp <= 180.0
                        } else { false }
                    } else { false };

                    // 多数派（候选整数）强制：不依赖切歌检测，只要最近候选多数明确则也允许显示
                    let allow_major = if !allow_hard && !allow_soft {
                        let nowh = now_ms();
                        let mut counts: std::collections::HashMap<i32, usize> = std::collections::HashMap::new();
                        for (v, t) in recent_ints_cand.iter().rev() { if nowh.saturating_sub(*t) <= 1200 { *counts.entry(*v).or_insert(0) += 1; } else { break; } }
                        let mut best: Option<(i32, usize)> = None;
                        for (k, c) in counts { if best.map_or(true, |(_, bc)| c > bc) { best = Some((k, c)); } }
                        if let Some((k, c)) = best {
                            let need = if in_fast { 2 } else { 3 };
                            let prev_int = (*guard).and_then(|g| Some(g.bpm.round() as i32)).unwrap_or(disp.round() as i32);
                            c >= need && k != prev_int && (60..=180).contains(&k)
                        } else { false }
                    } else { false };

                    // 允许显示（硬门或软门或多数派）。若软门整数与上次高亮整数一致，则继续高亮显示
                    if allow_hard || allow_soft || allow_major {
                        let mut show_state = if allow_soft { "uncertain" } else { state };
                        let disp_int = disp.round() as i32;
                        if allow_soft {
                            if let Some(prev_int) = last_hard_int { if prev_int == disp_int { show_state = last_hard_state.unwrap_or("tracking"); } }
                            // 快速重锁期内：若与“上一显示值（即使是灰显）”的整数不同，优先允许更新，避免被旧整数粘住
                            if in_fast {
                                if let Some(prev) = *guard { if prev.bpm.round() as i32 != disp_int { show_state = "uncertain"; } }
                            }
                        }
                        // 主导整数强制：基于“候选整数”的最近1.5s众数（而不是已显示整数）
                        // 阈值：快速期≥2；常态≥3（不依赖切歌检测也可切换）
                        let payload = {
                            let nowh = now_ms();
                            let mut counts: std::collections::HashMap<i32, usize> = std::collections::HashMap::new();
                            for (v, t) in recent_ints_cand.iter().rev() { if nowh.saturating_sub(*t) <= 1500 { *counts.entry(*v).or_insert(0) += 1; } else { break; } }
                            let mut best: Option<(i32, usize)> = None;
                            for (k, c) in counts { if best.map_or(true, |(_, bc)| c > bc) { best = Some((k, c)); } }
                            if let Some((k, c)) = best {
                                let prev_int = (*guard).and_then(|g| Some(g.bpm.round() as i32)).unwrap_or(disp_int);
                                let need = if in_fast { 2 } else { 3 };
                                if (allow_soft || allow_hard || allow_major) && c >= need && k != prev_int { 
                                    // 友好提示：多数派导致的整数切换
                                    emit_friendly(&app, format!("依据多数候选切换整数至 {} BPM", k), format!("Switched to majority integer {} BPM", k));
                                    DisplayBpm { bpm: k as f32, confidence: conf, state: "uncertain", level }
                                }
                                else { DisplayBpm { bpm: disp, confidence: conf, state: show_state, level } }
                            } else { DisplayBpm { bpm: disp, confidence: conf, state: show_state, level } }
                        };
                    *guard = Some(payload);
                    let _ = app.emit_to("main", "bpm_update", payload);
                        // TTL 更新时间已在整数锁作用域内完成
                        // 记录最近一次高亮整数
                        if !allow_soft {
                            // 若显示整数变化，输出一次友好提示
                            let changed = match last_hard_int { Some(prev) => prev != disp_int, None => true };
                            if changed { emit_friendly(&app, format!("当前节拍：{} BPM", disp_int), format!("Current tempo: {} BPM", disp_int)); }
                            last_hard_int = Some(disp_int); last_hard_state = Some(state);
                        }
                        // 记录近期整数（用于快速主导整数判定）
                        let nowh = now_ms();
                        recent_ints.push_back((payload.bpm.round() as i32, nowh));
                        while let Some(&(_, t0)) = recent_ints.front() { if nowh.saturating_sub(t0) > 1500 { recent_ints.pop_front(); } else { break; } }
                    }
                }
            }
            // 高置信度时的锚点门控更新：仅在显示值处于 60–160 且与锚点相对误差≤8% 时更新
            if tracking && conf >= 0.85 {
                if let Some(base) = anchor_bpm {
                    let rel = (disp - base).abs() / base.max(1e-6);
                    if rel <= 0.08 && (60.0..=160.0).contains(&disp) {
                        anchor_bpm = Some(base * 0.85 + disp * 0.15);
                        if let Some(a) = anchor_bpm { if EMIT_TEXT_LOGS { let txt = if is_log_zh() { format!("[锚点] 更新 -> {:.1}", a) } else { format!("[ANCHOR] anchor_bpm(update) -> {:.1}", a) }; eprintln!("{}", txt); let _ = app.emit_to("main", "bpm_log", BackendLog { t_ms: now_ms(), msg: txt }); } emit_friendly(&app, format!("更新参考节拍：{:.1} BPM", a), format!("Anchor updated: {:.1} BPM", a)); }
                    }
                } else {
                    if (60.0..=160.0).contains(&disp) {
                        anchor_bpm = Some(disp);
                        if EMIT_TEXT_LOGS { let txt = if is_log_zh() { format!("[锚点] 初始化 -> {:.1}", disp) } else { format!("[ANCHOR] anchor_bpm(init) -> {:.1}", disp) }; eprintln!("{}", txt); let _ = app.emit_to("main", "bpm_log", BackendLog { t_ms: now_ms(), msg: txt }); } emit_friendly(&app, format!("建立参考节拍：{:.1} BPM", disp), format!("Anchor set: {:.1} BPM", disp));
                    }
                }
            }
            // 滑动步进
            for _ in 0..hop_len { let _ = window.pop_front(); }

            // 分析日志（每2秒一行）：窗口长度、来源、bpm、置信度、当前状态
            // 仅输出前端会显示的 BPM 日志（置信度达到门槛）
            if conf >= 0.80 && EMIT_TEXT_LOGS {
            let src = if raw.from_short { "S" } else { "L" };
                let txt = if is_log_zh() {
                    format!("[显示] 窗口={:.1}s 源={} bpm={:.1} 置信度={:.2} 状态={} 电平={:.2}", raw.win_sec, src, disp, conf, state, level)
                } else {
                    format!("[SHOW] win={:.1}s src={} bpm={:.1} conf={:.2} state={} lvl={:.2}", raw.win_sec, src, disp, conf, state, level)
                };
                eprintln!("{}", txt);
                let log = BackendLog { t_ms: now_ms(), msg: txt.clone() };
                let _ = app.emit_to("main", "bpm_log", log.clone());
                if let Some(cell) = COLLECTED_LOGS.get() { if let Ok(mut g) = cell.lock() { g.push(log); } }
            }

            // （已移除 bpm_debug 事件收集）
        } else {
            none_cnt += 1;
            if none_cnt >= 6 { tracking = false; ever_locked = false; }
            if let Some(cell) = CURRENT_BPM.get() {
                if let Ok(mut guard) = cell.lock() {
                    let payload = if let Some(last) = *guard {
                        let state = if ever_locked { "uncertain" } else { "analyzing" };
                        DisplayBpm { bpm: last.bpm, confidence: 0.0, state, level }
                    } else {
                        DisplayBpm { bpm: 0.0, confidence: 0.0, state: "analyzing", level }
                    };
                    *guard = Some(payload);
                    let _ = app.emit_to("main", "bpm_update", payload);
                }
            }
            // 文本日志：无有效估计
            if EMIT_TEXT_LOGS {
                let txt = if is_log_zh() { format!("[无结果] 本步无估计 电平={:.2} 追踪={} 连续空帧={}", level, tracking, none_cnt) } else { format!("[NONE] win step no estimate, lvl={:.2} tracking={} none_cnt={}", level, tracking, none_cnt) };
                eprintln!("{}", txt);
                let log = BackendLog { t_ms: now_ms(), msg: txt.clone() };
                let _ = app.emit_to("main", "bpm_log", log.clone());
                if let Some(cell) = COLLECTED_LOGS.get() { if let Ok(mut g) = cell.lock() { g.push(log); } }
            }
            // 友好提示：尚未检测到清晰节拍
            emit_friendly(&app, "暂未检测到清晰节拍，继续聆听…", "No clear beat yet. Listening…");
            // （已移除 bpm_debug 事件收集）
            // 滑动步进
            for _ in 0..hop_len { let _ = window.pop_front(); }
            // 标记：经历较长 none 段后，下一次成功估计进入快速重锁期
            if none_cnt >= 6 { recent_none_flag = true; }
        }
    }
}

#[tauri::command]
fn stop_capture() -> Result<(), String> { Ok(()) }

fn main() {
    // 超早期日志，捕捉初始化前的崩溃
    early_setup_logging();
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
        .setup(|app| {
            let handle = app.handle();
            setup_logging(&handle);
            // 输出一次当前日志语言（用于确认）
            if is_log_zh() { append_log_line("[LANG] 日志语言=中文"); eprintln!("[语言] 日志输出：中文"); } else { append_log_line("[LANG] log language=EN"); eprintln!("[LANG] log language: EN"); }
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
                                #[cfg(debug_assertions)]
                                let url = Url::parse("http://localhost:5173/#logs").unwrap();
                                #[cfg(not(debug_assertions))]
                                let url = Url::parse("tauri://localhost/index.html#logs").unwrap();
                                let title = if is_log_zh() { "分析日志" } else { "Logs" };
                                let _ = WebviewWindowBuilder::new(app, "logs", WebviewUrl::External(url))
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
                                #[cfg(debug_assertions)]
                                let url = Url::parse("http://localhost:5173/#about").unwrap();
                                #[cfg(not(debug_assertions))]
                                let url = Url::parse("tauri://localhost/index.html#about").unwrap();
                                let _ = WebviewWindowBuilder::new(app, "about", WebviewUrl::External(url))
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

            // 开发模式下显式导航至 Vite 开发服务器，避免资源协议映射异常
            #[cfg(debug_assertions)]
            {
                let app_handle = app.handle().clone();
                std::thread::spawn(move || {
                    // 最多等待 ~2.5 秒，直到主窗口可获取
                    for _ in 0..50 {
                        if let Some(win) = app_handle.get_webview_window("main") {
                            if let Ok(url) = Url::parse("http://localhost:5173") {
                                let _ = win.navigate(url);
                            }
                            break;
                        }
                        std::thread::sleep(std::time::Duration::from_millis(50));
                    }
                });
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![start_capture, stop_capture, get_current_bpm, set_always_on_top, get_updater_endpoints, get_log_dir, reset_backend, set_log_lang, get_log_lang])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
