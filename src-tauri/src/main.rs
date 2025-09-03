#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod audio;
mod bpm;
mod tempo;

use std::{thread, time::Duration, sync::{Mutex, OnceLock}};
use std::collections::VecDeque;

use anyhow::Result;
use serde::{Serialize, Deserialize};
use tauri::{AppHandle, Manager, Emitter};
use tauri_plugin_single_instance::init as single_instance;

use audio::AudioService;
use tempo::{make_backend, TempoBackend};

#[derive(Serialize, Clone, Copy)]
struct DisplayBpm { bpm: f32, confidence: f32, state: &'static str, level: f32 }

#[derive(Serialize, Clone)]
struct BackendLog { t_ms: u64, msg: String }

#[derive(Serialize, Clone, Copy)]
struct BpmDebug {
    // 调试事件：时间戳（ms）与阶段
    t_ms: u64,
    phase: &'static str, // estimate | silent | none
    // 能量与基本上下文
    level: f32,
    state: &'static str,
    tracking: bool,
    ever_locked: bool,
    hi_cnt: usize,
    lo_cnt: usize,
    none_cnt: usize,
    anchor_bpm: Option<f32>,
    sample_rate: u32,
    hop_len: usize,
    // 估计相关（仅在 estimate 时有值）
    raw_bpm: Option<f32>,
    raw_confidence: Option<f32>,
    raw_rms: Option<f32>,
    from_short: Option<bool>,
    win_sec: Option<f32>,
    disp_bpm: Option<f32>,
    smoothed_bpm: Option<f32>,
    corr: Option<&'static str>, // raw | half | dbl
}

#[derive(Serialize, Clone)]
struct AudioViz {
    // 下采样后的波形样本，范围约 [-1, 1]
    samples: Vec<f32>,
    // 当前包的 RMS（0-1）
    rms: f32,
}

#[derive(Deserialize)]
struct FrontendBundle {
    frontend_debug: Vec<serde_json::Value>,
    frontend_logs: Vec<serde_json::Value>,
    frontend_updates: Vec<serde_json::Value>,
}

static CURRENT_BPM: OnceLock<Mutex<Option<DisplayBpm>>> = OnceLock::new();
static COLLECTED_DEBUG: OnceLock<Mutex<Vec<serde_json::Value>>> = OnceLock::new();
static COLLECTED_LOGS: OnceLock<Mutex<Vec<BackendLog>>> = OnceLock::new();

#[tauri::command]
fn start_capture(app: AppHandle) -> Result<(), String> {
    let _ = CURRENT_BPM.set(Mutex::new(None));
    let _ = COLLECTED_DEBUG.set(Mutex::new(Vec::new()));
    let _ = COLLECTED_LOGS.set(Mutex::new(Vec::new()));
    thread::spawn(move || {
        if let Err(_e) = run_capture(app) { }
    });
    Ok(())
}

#[tauri::command]
fn get_current_bpm() -> Option<DisplayBpm> {
    CURRENT_BPM.get().and_then(|m| m.lock().ok().and_then(|g| *g))
}
#[tauri::command]
fn export_debug() -> Result<String, String> {
    let dbg = if let Some(m) = COLLECTED_DEBUG.get() { if let Ok(g) = m.lock() { g.clone() } else { Vec::new() } } else { Vec::new() };
    let logs = if let Some(m) = COLLECTED_LOGS.get() { if let Ok(g) = m.lock() { g.clone() } else { Vec::new() } } else { Vec::new() };
    let payload = serde_json::json!({
        "meta": { "ts": now_ms(), "app": "bpm-sniffer" },
        "backend_debug": dbg,
        "backend_logs": logs,
    });
    serde_json::to_string_pretty(&payload).map_err(|e| e.to_string())
}

#[tauri::command]
fn export_debug_merged(frontend: FrontendBundle) -> Result<String, String> {
    let dbg = if let Some(m) = COLLECTED_DEBUG.get() { if let Ok(g) = m.lock() { g.clone() } else { Vec::new() } } else { Vec::new() };
    let logs = if let Some(m) = COLLECTED_LOGS.get() { if let Ok(g) = m.lock() { g.clone() } else { Vec::new() } } else { Vec::new() };
    let payload = serde_json::json!({
        "meta": { "ts": now_ms(), "app": "bpm-sniffer" },
        "backend_debug": dbg,
        "backend_logs": logs,
        "frontend_debug": frontend.frontend_debug,
        "frontend_logs": frontend.frontend_logs,
        "frontend_updates": frontend.frontend_updates,
    });
    // 写入桌面目录
    let home = std::env::var("USERPROFILE").or_else(|_| std::env::var("HOME")).map_err(|e| e.to_string())?;
    let desktop = std::path::Path::new(&home).join("Desktop");
    let fname = format!("bpm-debug-{}.json", now_ms());
    let path = desktop.join(fname);
    std::fs::write(&path, serde_json::to_string_pretty(&payload).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    Ok(path.to_string_lossy().to_string())
}

#[tauri::command]
fn set_always_on_top(app: AppHandle, on_top: bool) -> Result<(), String> {
    if let Some(win) = app.get_webview_window("main") {
        win.set_always_on_top(on_top).map_err(|e| e.to_string())?;
        Ok(())
    } else {
        Err("window not found".into())
    }
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
    let (svc, rx) = AudioService::start_loopback()?;

    let mut backend: Box<dyn TempoBackend> = make_backend(svc.sample_rate());

    let hi_th = 0.40f32; // 更快进入 tracking
    let lo_th = 0.25f32;
    let mut hi_cnt = 0usize;
    let mut lo_cnt = 0usize;
    let mut tracking = false;

    let mut ever_locked = false;
    let mut none_cnt = 0usize;

    // 滑动窗口：窗口 2s，步长 0.5s（重叠 75%）
    let sr_usize = svc.sample_rate() as usize;
    let target_len = sr_usize * 2;
    let hop_len = sr_usize / 2;
    let mut window: VecDeque<f32> = VecDeque::with_capacity(target_len * 2);
    let mut no_data_ms: u64 = 0;
    let mut silent_win_cnt: usize = 0;
    let mut anchor_bpm: Option<f32> = None; // 高置信度时的锚点，用于半/倍频纠偏
    // 显示平滑缓存（稳定优先：时间窗口中值 + EMA）
    let mut disp_hist: VecDeque<f32> = VecDeque::with_capacity(7);
    let mut ema_disp: Option<f32> = None;
    // 稳定聚合窗口（毫秒）：优先稳定，牺牲一定延迟
    let stable_win_ms: u64 = 600;
    let mut stable_vals: VecDeque<(f32, u64)> = VecDeque::with_capacity(256);

    loop {
        // 仅 Simple；阻塞接收，直至累积 ≥ 窗口大小（带超时）
        while window.len() < target_len {
            match rx.recv_timeout(Duration::from_millis(20)) {
                Ok(mut buf) => {
                    // 先基于当前包生成可视化，再将其推入窗口，避免 drain 后访问空缓冲
                    let out_len = 256usize;
                    if !buf.is_empty() {
                        let len = buf.len();
                        let mut rms_acc = 0.0f32;
                        for &v in &buf { rms_acc += v * v; }
                        let rms = (rms_acc / len as f32).sqrt().min(1.0);
                        let step = (len as f32 / out_len as f32).max(1.0);
                        let mut out: Vec<f32> = Vec::with_capacity(out_len);
                        let mut idx_f = 0.0f32;
                        for _ in 0..out_len {
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
                        let _ = app.emit_to("main", "viz_update", AudioViz { samples: out, rms });
                    } else {
                        let _ = app.emit_to("main", "viz_update", AudioViz { samples: vec![0.0; out_len], rms: 0.0 });
                    }

                    // 再推入分析窗口
                    for s in buf.drain(..) { window.push_back(s); }
                    no_data_ms = 0;
                }
                Err(_) => {
                    no_data_ms += 20;
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
        // 取窗口前 2s 作为当前帧
        let mut frames: Vec<f32> = Vec::with_capacity(target_len);
        for i in 0..target_len { if let Some(&v) = window.get(i) { frames.push(v); } }

        let level = level_from_frames(&frames);
        let is_silent = level < 0.03; // 略提高阈值，加速静音判定

        if is_silent {
            // 无声：直接显示 0 BPM，清空锁定状态
            tracking = false; ever_locked = false; hi_cnt = 0; lo_cnt = 0;
            silent_win_cnt = silent_win_cnt.saturating_add(1);
            if let Some(cell) = CURRENT_BPM.get() {
                if let Ok(mut guard) = cell.lock() {
                    let payload = DisplayBpm { bpm: 0.0, confidence: 0.0, state: "analyzing", level };
                    *guard = Some(payload);
                    let _ = app.emit_to("main", "bpm_update", payload);
                }
            }
            // 静音调试事件
            let dbg = BpmDebug {
                t_ms: now_ms(),
                phase: "silent",
                level,
                state: "analyzing",
                tracking,
                ever_locked,
                hi_cnt,
                lo_cnt,
                none_cnt,
                anchor_bpm,
                sample_rate: svc.sample_rate(),
                hop_len,
                raw_bpm: None,
                raw_confidence: None,
                raw_rms: None,
                from_short: None,
                win_sec: None,
                disp_bpm: None,
                smoothed_bpm: None,
                corr: None,
            };
            let _ = app.emit_to("main", "bpm_debug", dbg);
            if let Some(cell) = COLLECTED_DEBUG.get() { if let Ok(mut g) = cell.lock() { let _ = g.push(serde_json::to_value(&dbg).unwrap_or(serde_json::Value::Null)); } }
            disp_hist.clear(); ema_disp = None;
            // 滑动步进
            for _ in 0..hop_len { let _ = window.pop_front(); }
            continue;
        }
        silent_win_cnt = 0;

        if let Some(raw) = backend.process(&frames) {
            none_cnt = 0;
            let mut conf = raw.confidence.min(0.9);
            conf = conf.powf(0.9);

            // 对 aubio 的超短窗（win_sec 很小）采用更宽松的追踪阈值，避免长期停留在 analyzing
            let is_ultra_short = raw.win_sec <= 0.1;
            let (thr_hi, thr_lo) = if is_ultra_short { (0.15f32, 0.08f32) } else { (hi_th, lo_th) };

            if conf >= thr_hi { hi_cnt += 1; } else { hi_cnt = 0; }
            if conf <= thr_lo { lo_cnt += 1; } else { lo_cnt = 0; }

            if !tracking && hi_cnt >= 3 {
                tracking = true; ever_locked = true;
                let txt = format!("[STATE] tracking=ON  bpm={:.1} conf={:.2}", raw.bpm, conf);
                eprintln!("{}", txt);
                let log = BackendLog { t_ms: now_ms(), msg: txt.clone() };
                let _ = app.emit_to("main", "bpm_log", log.clone());
                if let Some(cell) = COLLECTED_LOGS.get() { if let Ok(mut g) = cell.lock() { g.push(log); } }
            }
            if tracking && lo_cnt >= 2 {
                tracking = false;
                let txt = format!("[STATE] tracking=OFF conf={:.2}", conf);
                eprintln!("{}", txt);
                let log = BackendLog { t_ms: now_ms(), msg: txt.clone() };
                let _ = app.emit_to("main", "bpm_log", log.clone());
                if let Some(cell) = COLLECTED_LOGS.get() { if let Ok(mut g) = cell.lock() { g.push(log); } }
            }

            let state: &str = if tracking { "tracking" } else if ever_locked { "uncertain" } else { "analyzing" };

            // 锚点纠偏候选：{raw, 1/2x, 2x}
            let mut disp = raw.bpm;
            let mut corr_kind: &'static str = "raw";
            if let Some(base) = anchor_bpm {
                let mut best_bpm = disp;
                let mut best_err = (disp - base).abs();
                let mut best_kind: &'static str = "raw";

                let try_cand = |val: f32, kind: &'static str, best_bpm: &mut f32, best_err: &mut f32, best_kind: &mut &'static str| {
                    if val >= 60.0 && val <= 200.0 {
                        let err = (val - base).abs();
                        if err < *best_err - 1.0 { *best_err = err; *best_bpm = val; *best_kind = kind; }
                    }
                };

                // 候选集合
                try_cand(disp * 0.5, "half", &mut best_bpm, &mut best_err, &mut best_kind);
                try_cand(disp * 2.0, "dbl", &mut best_bpm, &mut best_err, &mut best_kind);

                if best_bpm != disp { eprintln!("[CORR] {} -> {:.1} (base={:.1}, raw={:.1})", best_kind, best_bpm, base, disp); }
                disp = best_bpm; corr_kind = best_kind;
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

            if let Some(cell) = CURRENT_BPM.get() {
                if let Ok(mut guard) = cell.lock() {
                    // UI 按“新值即刻显示，未更新保持上次”策略，这里直接发送未经平滑的纠偏值
                    let payload = DisplayBpm { bpm: disp, confidence: conf, state, level };
                    *guard = Some(payload);
                    let _ = app.emit_to("main", "bpm_update", payload);
                }
            }
            // 高置信度时的锚点门控更新：仅在显示值处于 60–160 且与锚点相对误差≤8% 时更新
            if tracking && conf >= 0.85 {
                if let Some(base) = anchor_bpm {
                    let rel = (disp - base).abs() / base.max(1e-6);
                    if rel <= 0.08 && (60.0..=160.0).contains(&disp) {
                        anchor_bpm = Some(base * 0.85 + disp * 0.15);
                        if let Some(a) = anchor_bpm {
                            let txt = format!("[ANCHOR] anchor_bpm(update) -> {:.1}", a);
                            eprintln!("{}", txt);
                            let _ = app.emit_to("main", "bpm_log", BackendLog { t_ms: now_ms(), msg: txt });
                        }
                    }
                } else {
                    if (60.0..=160.0).contains(&disp) {
                        anchor_bpm = Some(disp);
                        let txt = format!("[ANCHOR] anchor_bpm(init) -> {:.1}", disp);
                        eprintln!("{}", txt);
                        let _ = app.emit_to("main", "bpm_log", BackendLog { t_ms: now_ms(), msg: txt });
                    }
                }
            }
            // 滑动步进
            for _ in 0..hop_len { let _ = window.pop_front(); }

            // 分析日志（每2秒一行）：窗口长度、来源、bpm、置信度、当前状态
            let src = if raw.from_short { "S" } else { "L" };
            let txt = format!(
                "[ANA] win={:.1}s src={} bpm={:.1} conf={:.2} state={} lvl={:.2}",
                raw.win_sec, src, raw.bpm, conf, state, level
            );
            eprintln!("{}", txt);
            let log = BackendLog { t_ms: now_ms(), msg: txt.clone() };
            let _ = app.emit_to("main", "bpm_log", log.clone());
            if let Some(cell) = COLLECTED_LOGS.get() { if let Ok(mut g) = cell.lock() { g.push(log); } }

            // 结构化调试事件（前端可视化与导出）
            let dbg = BpmDebug {
                t_ms: now_ms(),
                phase: "estimate",
                level,
                state,
                tracking,
                ever_locked,
                hi_cnt,
                lo_cnt,
                none_cnt,
                anchor_bpm,
                sample_rate: svc.sample_rate(),
                hop_len,
                raw_bpm: Some(raw.bpm),
                raw_confidence: Some(conf),
                raw_rms: Some(raw.rms),
                from_short: Some(raw.from_short),
                win_sec: Some(raw.win_sec),
                disp_bpm: Some(disp),
                smoothed_bpm: Some(smoothed),
                corr: Some(corr_kind),
            };
            // 发送并收集
            let _ = app.emit_to("main", "bpm_debug", dbg);
            if let Some(cell) = COLLECTED_DEBUG.get() { if let Ok(mut g) = cell.lock() { let _ = g.push(serde_json::to_value(&dbg).unwrap_or(serde_json::Value::Null)); } }
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
            let txt = format!("[NONE] win step no estimate, lvl={:.2} tracking={} none_cnt={}", level, tracking, none_cnt);
            eprintln!("{}", txt);
            let log = BackendLog { t_ms: now_ms(), msg: txt.clone() };
            let _ = app.emit_to("main", "bpm_log", log.clone());
            if let Some(cell) = COLLECTED_LOGS.get() { if let Ok(mut g) = cell.lock() { g.push(log); } }
            // 无估计：发送调试事件
            let dbg = BpmDebug {
                t_ms: now_ms(),
                phase: "none",
                level,
                state: if ever_locked { "uncertain" } else { "analyzing" },
                tracking,
                ever_locked,
                hi_cnt,
                lo_cnt,
                none_cnt,
                anchor_bpm,
                sample_rate: svc.sample_rate(),
                hop_len,
                raw_bpm: None,
                raw_confidence: None,
                raw_rms: None,
                from_short: None,
                win_sec: None,
                disp_bpm: None,
                smoothed_bpm: None,
                corr: None,
            };
            let _ = app.emit_to("main", "bpm_debug", dbg);
            if let Some(cell) = COLLECTED_DEBUG.get() { if let Ok(mut g) = cell.lock() { let _ = g.push(serde_json::to_value(&dbg).unwrap_or(serde_json::Value::Null)); } }
            // 滑动步进
            for _ in 0..hop_len { let _ = window.pop_front(); }
        }
    }
}

#[tauri::command]
fn stop_capture() -> Result<(), String> { Ok(()) }

fn main() {
    // 开发环境：将 DLL 搜索目录加入进程 PATH，便于加载 src-tauri/bin/windows/x64 下的依赖
    #[cfg(windows)]
    {
        use std::env;
        use std::path::PathBuf;
        let mut paths: Vec<PathBuf> = env::split_paths(&env::var_os("PATH").unwrap_or_default()).collect();
        let dev_bin = PathBuf::from("./src-tauri/bin/windows/x64");
        if dev_bin.exists() { paths.insert(0, dev_bin); }
        let resources_bin = PathBuf::from("./bin/windows/x64");
        if resources_bin.exists() { paths.insert(0, resources_bin); }
        let merged = env::join_paths(paths).ok();
        if let Some(p) = merged { env::set_var("PATH", p); }
    }

    tauri::Builder::default()
        .plugin(single_instance(|app, _args, _cwd| {
            if let Some(win) = app.get_webview_window("main") { let _ = win.set_focus(); }
        }))
        .invoke_handler(tauri::generate_handler![start_capture, stop_capture, get_current_bpm, export_debug, export_debug_merged, set_always_on_top])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
