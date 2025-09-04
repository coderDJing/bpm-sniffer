#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod audio;
mod bpm;
mod tempo;

use std::{thread, time::Duration, sync::{Mutex, OnceLock}};
use std::collections::VecDeque;

use anyhow::Result;
use serde::Serialize;
use tauri::{AppHandle, Manager, Emitter};
use tauri_plugin_single_instance::init as single_instance;
// use tauri_plugin_updater::UpdaterExt;
use tauri::menu::{MenuBuilder, MenuItemBuilder};
use tauri::tray::TrayIconBuilder;
use tauri::Url;
use tauri::webview::WebviewWindowBuilder;
use tauri::WebviewUrl;

use audio::AudioService;
use tempo::{make_backend, TempoBackend};

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
const EMIT_TEXT_LOGS: bool = true;

#[tauri::command]
fn start_capture(app: AppHandle) -> Result<(), String> {
    let _ = CURRENT_BPM.set(Mutex::new(None));
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
    loop {
        // 仅 Simple；阻塞接收，直至累积 ≥ 窗口大小（带超时）
        while window.len() < target_len {
            match rx.recv_timeout(Duration::from_millis(20)) {
                Ok(mut buf) => {
                    // 先基于当前包生成可视化，再将其推入窗口，避免 drain 后访问空缓冲
                    let out_len = 192usize; // 降低单帧数据量以提升帧率
                    if !buf.is_empty() {
                        let len = buf.len();
                        let mut rms_acc = 0.0f32;
                        for &v in &buf { rms_acc += v * v; }
                        let rms = (rms_acc / len as f32).sqrt().min(1.0);
                        // 可视化 RMS：对极低电平直接视为静音，立即归零
                        let silent_cut = 0.015f32;
                        let viz_rms = if rms < silent_cut { 0.0 } else { rms };
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
                        let nowv = now_ms();
                        if nowv.saturating_sub(last_viz_ms) >= 33 {
                            let _ = app.emit_to("main", "viz_update", AudioViz { samples: out, rms: viz_rms });
                            last_viz_ms = nowv;
                        }
                    } else {
                        let nowv = now_ms();
                        if nowv.saturating_sub(last_viz_ms) >= 33 {
                            let _ = app.emit_to("main", "viz_update", AudioViz { samples: vec![0.0; out_len], rms: 0.0 });
                            last_viz_ms = nowv;
                        }
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
            // 滑动步进
            for _ in 0..hop_len { let _ = window.pop_front(); }
            continue;
        }
        silent_win_cnt = 0;

        if let Some(raw) = backend.process(&frames) {
            none_cnt = 0;
            // 能量跳变触发切歌快速重锁
            if let Some(pdb) = prev_rms_db { if (cur_db - pdb).abs() >= 6.0 { fast_relock_deadline = Some(now_ms().saturating_add(2000)); anchor_bpm = None; stable_vals.clear(); } }
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
                    let txt = format!("[STATE] tracking=ON  bpm={:.1} conf={:.2}", raw.bpm, conf);
                    eprintln!("{}", txt);
                    let log = BackendLog { t_ms: now_ms(), msg: txt.clone() };
                    let _ = app.emit_to("main", "bpm_log", log.clone());
                    if let Some(cell) = COLLECTED_LOGS.get() { if let Ok(mut g) = cell.lock() { g.push(log); } }
                }
            }
            if tracking && lo_cnt >= 2 {
                tracking = false;
                if EMIT_TEXT_LOGS {
                    let txt = format!("[STATE] tracking=OFF conf={:.2}", conf);
                    eprintln!("{}", txt);
                    let log = BackendLog { t_ms: now_ms(), msg: txt.clone() };
                    let _ = app.emit_to("main", "bpm_log", log.clone());
                    if let Some(cell) = COLLECTED_LOGS.get() { if let Ok(mut g) = cell.lock() { g.push(log); } }
                }
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

                if best_bpm != disp && EMIT_TEXT_LOGS { eprintln!("[CORR] {} -> {:.1} (base={:.1}, raw={:.1})", best_kind, best_bpm, base, disp); }
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
            if recent_none_flag && conf >= 0.50 { fast_relock_deadline = Some(now_ms().saturating_add(2000)); anchor_bpm = None; recent_none_flag = false; stable_vals.clear(); }
            if dev_from_lock_cnt >= 2 {
                fast_relock_deadline = Some(now_ms().saturating_add(2000));
                anchor_bpm = None; dev_from_lock_cnt = 0; stable_vals.clear();
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
                                if err <= 0.08 { disp = best; } else { allow_hard = false; if EMIT_TEXT_LOGS { eprintln!("[OUTLIER] suppress show bpm={:.1} base={:.1} rel={:.3}", disp, base, rel); } }
                            }
                        }}
                        // 范围限幅：若候选超出 91–180 且存在基准，则直接抑制
                        if allow_hard {
                            if let Some(base) = base_for_guard {
                                if (disp > 180.0 || disp < 91.0) && conf >= 0.80 {
                                    allow_hard = false;
                                    if EMIT_TEXT_LOGS { eprintln!("[OUTLIER] suppress out-of-range bpm={:.1} base={:.1}", disp, base); }
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
                                if (allow_soft || allow_hard || allow_major) && c >= need && k != prev_int { DisplayBpm { bpm: k as f32, confidence: conf, state: "uncertain", level } }
                                else { DisplayBpm { bpm: disp, confidence: conf, state: show_state, level } }
                            } else { DisplayBpm { bpm: disp, confidence: conf, state: show_state, level } }
                        };
                    *guard = Some(payload);
                    let _ = app.emit_to("main", "bpm_update", payload);
                        // TTL 更新时间已在整数锁作用域内完成
                        // 记录最近一次高亮整数
                        if !allow_soft { last_hard_int = Some(disp_int); last_hard_state = Some(state); }
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
                        if let Some(a) = anchor_bpm { if EMIT_TEXT_LOGS { let txt = format!("[ANCHOR] anchor_bpm(update) -> {:.1}", a); eprintln!("{}", txt); let _ = app.emit_to("main", "bpm_log", BackendLog { t_ms: now_ms(), msg: txt }); } }
                    }
                } else {
                    if (60.0..=160.0).contains(&disp) {
                        anchor_bpm = Some(disp);
                        if EMIT_TEXT_LOGS { let txt = format!("[ANCHOR] anchor_bpm(init) -> {:.1}", disp); eprintln!("{}", txt); let _ = app.emit_to("main", "bpm_log", BackendLog { t_ms: now_ms(), msg: txt }); }
                    }
                }
            }
            // 滑动步进
            for _ in 0..hop_len { let _ = window.pop_front(); }

            // 分析日志（每2秒一行）：窗口长度、来源、bpm、置信度、当前状态
            // 仅输出前端会显示的 BPM 日志（置信度达到门槛）
            if conf >= 0.80 && EMIT_TEXT_LOGS {
            let src = if raw.from_short { "S" } else { "L" };
                let txt = format!(
                    "[SHOW] win={:.1}s src={} bpm={:.1} conf={:.2} state={} lvl={:.2}",
                    raw.win_sec, src, disp, conf, state, level
                );
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
                let txt = format!("[NONE] win step no estimate, lvl={:.2} tracking={} none_cnt={}", level, tracking, none_cnt);
                eprintln!("{}", txt);
                let log = BackendLog { t_ms: now_ms(), msg: txt.clone() };
                let _ = app.emit_to("main", "bpm_log", log.clone());
                if let Some(cell) = COLLECTED_LOGS.get() { if let Ok(mut g) = cell.lock() { g.push(log); } }
            }
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
    tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_opener::init())
        .plugin(single_instance(|app, _args, _cwd| {
            if let Some(win) = app.get_webview_window("main") { let _ = win.set_focus(); }
        }))
        .setup(|app| {
            // 系统托盘与菜单
            let about = MenuItemBuilder::new("关于").id("about").build(app)?;
            let quit = MenuItemBuilder::new("退出").id("quit").build(app)?;
            let menu = MenuBuilder::new(app)
                .items(&[&about, &quit])
                .build()?;

            let icon = app.default_window_icon().cloned();
            let mut tray_builder = TrayIconBuilder::new()
                .menu(&menu)
                .on_menu_event(|app, event| {
                    match event.id().as_ref() {
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
                                    .title("关于 BPM Sniffer")
                                    .resizable(false)
                                    .inner_size(360.0, 280.0)
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
            tray_builder.build(app)?;

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
        .invoke_handler(tauri::generate_handler![start_capture, stop_capture, get_current_bpm, set_always_on_top])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
