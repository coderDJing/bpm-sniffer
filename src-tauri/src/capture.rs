use anyhow::Result;
use std::collections::VecDeque;
use std::time::Duration;

use tauri::{AppHandle, Emitter};

use crate::audio::AudioService;
use crate::lang::is_log_zh;
use crate::logging::{emit_friendly, now_ms, EMIT_TEXT_LOGS};
use crate::state::{AudioViz, BackendLog, DisplayBpm, COLLECTED_LOGS, CURRENT_BPM, OUT_LEN, RESET_REQUESTED};
use crate::tempo::{make_backend, TempoBackend};

// 分析支路响度标准化配置（与原 main.rs 一致）
const NORM_ENABLE: bool = true;
const NORM_TARGET_DBFS: f32 = -18.0;
const NORM_MAX_GAIN_DB: f32 = 36.0;
const NORM_MIN_GAIN_DB: f32 = -12.0;
const NORM_SOFT_K: f32 = 1.2;
const NORM_ATTACK: f32 = 0.25;
const NORM_RELEASE: f32 = 0.08;
const SC_HP_HZ: f32 = 60.0;
const SC_LP_HZ: f32 = 180.0;
const NORM_MAX_GAIN_DB_EXT: f32 = 42.0;
const RHYTHM_RATIO_THR: f32 = 0.25;
const MAX_GAIN_DB_WHEN_LOW_RATIO: f32 = 18.0;

fn level_from_frames(frames: &[f32]) -> f32 {
    if frames.is_empty() { return 0.0; }
    let mut sum = 0.0f32;
    for &s in frames { sum += s * s; }
    let rms = (sum / frames.len() as f32).sqrt();
    let db = 20.0 * (rms.max(1e-9)).log10();
    ((db + 60.0) / 60.0).clamp(0.0, 1.0)
}

pub fn run_capture(app: AppHandle) -> Result<()> {
    let (svc, rx, sr_rx) = AudioService::start_loopback()?;
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

    // 可视化事件节流：至少间隔 16ms 发送一次（~60fps）
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
    // 侧链滤波状态（简单一阶高通+低通）
    let mut sc_hp_alpha: f32 = 0.0;
    let mut sc_lp_alpha: f32 = 0.0;
    let mut sc_hp_lp_prev: f32 = 0.0;
    let mut sc_lp_prev: f32 = 0.0;

    loop {
        if let Some(flag) = RESET_REQUESTED.get() {
            if flag.swap(false, std::sync::atomic::Ordering::SeqCst) {
                window.clear();
                disp_hist.clear(); ema_disp = None; prev_rms_db = None; anchor_bpm = None; stable_vals.clear();
                hi_cnt = 0; lo_cnt = 0; tracking = false; ever_locked = false; none_cnt = 0; dev_from_lock_cnt = 0; force_clear_lock = true;
                recent_none_flag = false; last_hard_int = None; last_hard_state = None; trk_x = None; trk_v = 0.0;
                recent_ints.clear(); recent_ints_cand.clear(); noise_floor_rms = 0.01;
                let _ = app.emit_to("main", "viz_update", AudioViz { samples: vec![0.0; OUT_LEN], rms: 0.0 });
                if let Some(cell) = CURRENT_BPM.get() { if let Ok(mut guard) = cell.lock() { let payload = DisplayBpm { bpm: 0.0, confidence: 0.0, state: "analyzing", level: 0.0 }; *guard = Some(payload); let _ = app.emit_to("main", "bpm_update", payload); } }
            }
        }

        while window.len() < target_len {
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
                    let _ = app.emit("viz_update", AudioViz { samples: vec![0.0; OUT_LEN], rms: 0.0 });
                    if let Some(cell) = CURRENT_BPM.get() { if let Ok(mut guard) = cell.lock() { let payload = DisplayBpm { bpm: 0.0, confidence: 0.0, state: "analyzing", level: 0.0 }; *guard = Some(payload); let _ = app.emit("bpm_update", payload); } }
                }
            }
            match rx.recv_timeout(Duration::from_millis(20)) {
                Ok(mut buf) => {
                    if !buf.is_empty() {
                        let len = buf.len();
                        let mut rms_acc = 0.0f32;
                        for &v in &buf { rms_acc += v * v; }
                        let rms = (rms_acc / len as f32).sqrt().min(1.0);
                        let viz_rms = rms;
                        let nowv = now_ms();
                        if nowv.saturating_sub(last_viz_ms) >= 16 {
                            if rms < 0.015 {
                                let _ = app.emit_to("main", "viz_update", AudioViz { samples: vec![0.0; OUT_LEN], rms: viz_rms });
                            } else {
                                let step = (len as f32 / OUT_LEN as f32).max(1.0);
                                let mut out: Vec<f32> = Vec::with_capacity(OUT_LEN);
                                let mut idx_f = 0.0f32;
                                for _ in 0..OUT_LEN {
                                    let i0 = idx_f as usize;
                                    let i1 = ((idx_f + step) as usize).min(len);
                                    let mut acc = 0.0f32;
                                    let mut cnt = 0usize;
                                    if i0 < i1 {
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
                        if nowv.saturating_sub(last_viz_ms) >= 16 {
                            let _ = app.emit_to("main", "viz_update", AudioViz { samples: vec![0.0; OUT_LEN], rms: 0.0 });
                            last_viz_ms = nowv;
                        }
                    }
                    for s in buf.drain(..) { window.push_back(s); }
                    no_data_ms = 0;
                }
                Err(_) => {
                    no_data_ms += 20;
                    let nowv = now_ms();
                    if nowv.saturating_sub(last_viz_ms) >= 16 {
                        let _ = app.emit("viz_update", AudioViz { samples: vec![0.0; OUT_LEN], rms: 0.0 });
                        last_viz_ms = nowv;
                    }
                    if no_data_ms >= 1500 {
                        tracking = false; ever_locked = false; hi_cnt = 0; lo_cnt = 0;
                        if let Some(cell) = CURRENT_BPM.get() {
                            if let Ok(mut guard) = cell.lock() {
                                let payload = DisplayBpm { bpm: 0.0, confidence: 0.0, state: "analyzing", level: 0.0 };
                                *guard = Some(payload);
                                let _ = app.emit("bpm_update", payload);
                            }
                        }
                        no_data_ms = 0;
                    }
                }
            }
        }

        let mut frames: Vec<f32> = Vec::with_capacity(target_len);
        for i in 0..target_len { if let Some(&v) = window.get(i) { frames.push(v); } }

        // 初始化侧链滤波系数（首次）
        let mut sc_hp_alpha_local = sc_hp_alpha;
        let mut sc_lp_alpha_local = sc_lp_alpha;
        if sc_hp_alpha_local == 0.0 || sc_lp_alpha_local == 0.0 {
            let fs = svc.sample_rate() as f32;
            sc_hp_alpha_local = (-2.0 * std::f32::consts::PI * SC_HP_HZ / fs).exp();
            sc_lp_alpha_local = (-2.0 * std::f32::consts::PI * SC_LP_HZ / fs).exp();
            sc_hp_lp_prev = 0.0; sc_lp_prev = 0.0;
            sc_hp_alpha = sc_hp_alpha_local; sc_lp_alpha = sc_lp_alpha_local;
        }

        let level = level_from_frames(&frames);
        let mut sumsq = 0.0f32;
        for &s in &frames { sumsq += s * s; }
        let rms = (sumsq / frames.len() as f32).sqrt();
        let cur_db = 20.0 * (rms.max(1e-9)).log10();
        noise_floor_rms = noise_floor_rms * 0.99 + rms * 0.01;
        let is_silent = level < 0.03;

        if is_silent {
            tracking = false; ever_locked = false; hi_cnt = 0; lo_cnt = 0;
            norm_gain_db_smooth = 0.0;
            silent_win_cnt = silent_win_cnt.saturating_add(1);
            if let Some(cell) = CURRENT_BPM.get() {
                if let Ok(mut guard) = cell.lock() {
                    let payload = DisplayBpm { bpm: 0.0, confidence: 0.0, state: "analyzing", level };
                    *guard = Some(payload);
                    let _ = app.emit("bpm_update", payload);
                }
            }
            emit_friendly(&app, "检测到环境安静，BPM 为 0（等待声音）", "Silence detected. BPM is 0 (waiting for audio)");
            was_silent_flag = true;
            for _ in 0..hop_len { let _ = window.pop_front(); }
            continue;
        }
        if was_silent_flag {
            emit_friendly(&app, "检测到声音，开始分析…", "Audio detected. Analyzing…");
            was_silent_flag = false;
        }
        silent_win_cnt = 0;

        let mut frames_for_analysis: Vec<f32> = frames.clone();
        if NORM_ENABLE {
            let mut sumsq = 0.0f32; for &s in &frames_for_analysis { sumsq += s * s; }
            let rms = (sumsq / frames_for_analysis.len() as f32).sqrt().max(1e-9);
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
            let dyn_max_gain = if rhythm_ratio >= RHYTHM_RATIO_THR { NORM_MAX_GAIN_DB.max(NORM_MAX_GAIN_DB_EXT) } else { NORM_MAX_GAIN_DB.min(MAX_GAIN_DB_WHEN_LOW_RATIO) };
            if need_gain_db > dyn_max_gain { need_gain_db = dyn_max_gain; }
            if need_gain_db < NORM_MIN_GAIN_DB { need_gain_db = NORM_MIN_GAIN_DB; }
            let a = if need_gain_db > norm_gain_db_smooth { NORM_ATTACK } else { NORM_RELEASE };
            norm_gain_db_smooth = norm_gain_db_smooth * (1.0 - a) + need_gain_db * a;
            let lin_gain = 10f32.powf(norm_gain_db_smooth / 20.0);
            if lin_gain != 1.0 { for x in &mut frames_for_analysis { *x *= lin_gain; } }
            if NORM_SOFT_K > 0.0 { for x in &mut frames_for_analysis { let y = (NORM_SOFT_K * *x).tanh(); *x = y / NORM_SOFT_K; } }
        }

        if let Some(raw) = backend.process(&frames_for_analysis) {
            none_cnt = 0;
            if last_from_short.map_or(true, |v| v != raw.from_short) {
                if raw.from_short { emit_friendly(&app, "切换为短窗优先（更快跟随变化）", "Switched to short-window (faster response)"); }
                else { emit_friendly(&app, "切换为长窗优先（更稳更准）", "Switched to long-window (more stable)"); }
                last_from_short = Some(raw.from_short);
            }
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
            let snr = (rms / noise_floor_rms.max(1e-6)).max(0.0);
            let snr_boost = (snr / 2.5).clamp(0.6, 1.15);
            conf = (conf * snr_boost).clamp(0.0, 0.95);

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
                emit_friendly(&app, format!("已锁定节拍：约 {:.0} BPM（稳定度 {:.0}%）", raw.bpm, conf*100.0), format!("Beat locked: ~{:.0} BPM (confidence {:.0}%)", raw.bpm, conf*100.0));
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
                emit_friendly(&app, "节拍暂不稳定，正在重新分析…", "Beat unstable. Re-analyzing…");
            }

            let state: &str = if tracking { "tracking" } else if ever_locked { "uncertain" } else { "analyzing" };
            let in_fast = fast_relock_deadline.map_or(false, |t| now_ms() < t);

            let mut disp = raw.bpm;
            let mut _corr_kind: &'static str = "raw";
            if let Some(base) = anchor_bpm {
                let mut best_bpm = disp;
                let mut best_err = (disp - base).abs();
                let mut best_kind: &'static str = "raw";

                let try_cand = |val: f32, kind: &'static str, best_bpm: &mut f32, best_err: &mut f32, best_kind: &mut &'static str| {
                    if val >= 60.0 && val <= 200.0 {
                        let err = (val - base).abs();
                        if err + 0.2 < *best_err { *best_err = err; *best_bpm = val; *best_kind = kind; }
                    }
                };

                try_cand(disp * 0.5, "half", &mut best_bpm, &mut best_err, &mut best_kind);
                try_cand(disp * 2.0, "dbl", &mut best_bpm, &mut best_err, &mut best_kind);
                try_cand(disp * (2.0/3.0), "two_thirds", &mut best_bpm, &mut best_err, &mut best_kind);
                try_cand(disp * (3.0/2.0), "three_halves", &mut best_bpm, &mut best_err, &mut best_kind);

                if best_bpm != disp && EMIT_TEXT_LOGS { if is_log_zh() { eprintln!("[谐波校正] {} -> {:.1} (基准={:.1}, 原始={:.1})", best_kind, best_bpm, base, disp); } else { eprintln!("[CORR] {} -> {:.1} (base={:.1}, raw={:.1})", best_kind, best_bpm, base, disp); } }
                if best_bpm != disp { emit_friendly(&app, format!("已纠正谐波：{} → {:.1}（参考 {:.1}，原始 {:.1}）", best_kind, best_bpm, base, disp), format!("Harmonic correction: {} → {:.1} (ref {:.1}, raw {:.1})", best_kind, best_bpm, base, disp)); }
                disp = best_bpm; _corr_kind = best_kind;
            }

            if disp < 91.0 || disp > 180.0 {
                let mut t = disp;
                for _ in 0..4 {
                    if t < 91.0 { t *= 2.0; }
                    else if t > 180.0 { t *= 0.5; }
                    else { break; }
                }
                if t >= 91.0 && t <= 180.0 { disp = t; _corr_kind = "edm_norm"; }
            }

            {
                static mut LOCK_INT: Option<i32> = None;
                static mut LOCK_CNT: u8 = 0;
                static mut UNLOCK_CNT: u8 = 0;
                static mut LAST_SHOW_MS: Option<u64> = None;
                static mut ALT_INT: Option<i32> = None;
                static mut ALT_CNT: u8 = 0;
                unsafe {
                    if let Some(last) = LAST_SHOW_MS { if now_ms().saturating_sub(last) > 10_000 { LOCK_INT = None; LOCK_CNT = 0; UNLOCK_CNT = 0; } }
                }
                let disp_round = disp.round() as i32;
                let diff = (disp - disp_round as f32).abs();
                let within = diff <= 0.6;
                unsafe {
                    if in_fast || force_clear_lock { LOCK_INT = None; LOCK_CNT = 0; UNLOCK_CNT = 0; ALT_INT = None; ALT_CNT = 0; force_clear_lock = false; }
                    if let Some(n) = LOCK_INT { if conf >= 0.85 && (disp - n as f32).abs() >= 2.0 { LOCK_INT = None; LOCK_CNT = 0; UNLOCK_CNT = 0; } }
                    if conf >= 0.80 && within {
                        if let Some(n) = LOCK_INT { if n == disp_round { LOCK_CNT = LOCK_CNT.saturating_add(1); } else { LOCK_CNT = 1; LOCK_INT = Some(disp_round); } }
                        else { LOCK_INT = Some(disp_round); LOCK_CNT = 1; }
                        if conf >= 0.90 && diff <= 0.4 { if LOCK_CNT < 2 { LOCK_CNT = 2; } }
                        if LOCK_CNT >= 2 { UNLOCK_CNT = 0; disp = disp_round as f32; }
                    } else if let Some(n) = LOCK_INT {
                        if conf >= 0.82 && (disp - n as f32).abs() > 1.3 {
                            UNLOCK_CNT = UNLOCK_CNT.saturating_add(1);
                            if UNLOCK_CNT >= 3 { LOCK_INT = None; LOCK_CNT = 0; UNLOCK_CNT = 0; }
                        } else { UNLOCK_CNT = 0; }
                        let switch_conf = if in_fast { 0.70 } else { 0.82 };
                        let switch_need = if in_fast { 2 } else { 3 };
                        if conf >= switch_conf {
                            let near_other = (disp - disp_round as f32).abs() <= 0.4 && disp_round != n;
                            if near_other {
                                if ALT_INT == Some(disp_round) { ALT_CNT = ALT_CNT.saturating_add(1); } else { ALT_INT = Some(disp_round); ALT_CNT = 1; }
                                if ALT_CNT as i32 >= switch_need { LOCK_INT = Some(disp_round); LOCK_CNT = 2; UNLOCK_CNT = 0; disp = disp_round as f32; }
                            } else { ALT_CNT = 0; }
                        } else { ALT_CNT = 0; }
                        if (disp - n as f32).abs() >= 8.0 { dev_from_lock_cnt = dev_from_lock_cnt.saturating_add(1); } else { dev_from_lock_cnt = 0; }
                    }
                }
                unsafe { if conf >= 0.80 { LAST_SHOW_MS = Some(now_ms()); } }
            }

            {
                let nowh = now_ms();
                recent_ints_cand.push_back((disp.round() as i32, nowh));
                while let Some(&(_, t0)) = recent_ints_cand.front() { if nowh.saturating_sub(t0) > 1500 { recent_ints_cand.pop_front(); } else { break; } }
            }

            if recent_none_flag && conf >= 0.50 {
                fast_relock_deadline = Some(now_ms().saturating_add(2000));
                anchor_bpm = None; recent_none_flag = false; stable_vals.clear();
                emit_friendly(&app, "从空段恢复，快速锁定中…", "Recovered from none, fast relock…");
            }
            if dev_from_lock_cnt >= 2 {
                fast_relock_deadline = Some(now_ms().saturating_add(2000));
                anchor_bpm = None; dev_from_lock_cnt = 0; stable_vals.clear();
                emit_friendly(&app, "检测到与锁定值偏离，快速锁定中…", "Deviation from locked value, fast relock…");
                let nowh = now_ms();
                let mut counts: std::collections::HashMap<i32, usize> = std::collections::HashMap::new();
                for (v, t) in recent_ints.iter().rev() { if nowh.saturating_sub(*t) <= 1500 { *counts.entry(*v).or_insert(0) += 1; } else { break; } }
                let mut best: Option<(i32, usize)> = None;
                for (k, c) in counts { if best.map_or(true, |(_, bc)| c > bc) { best = Some((k, c)); } }
                if let Some((_, c)) = best { if c >= 3 { force_clear_lock = true; } }
            }

            let now_t = now_ms();
            stable_vals.push_back((disp, now_t));
            while let Some(&(_, t0)) = stable_vals.front() { if now_t.saturating_sub(t0) > stable_win_ms { stable_vals.pop_front(); } else { break; } }
            let mut win_sorted: Vec<f32> = stable_vals.iter().map(|(v,_)| *v).collect();
            if win_sorted.is_empty() { win_sorted.push(disp); }
            win_sorted.sort_by(|a,b| a.partial_cmp(b).unwrap());
            let mid = win_sorted[win_sorted.len()/2];
            let smoothed = if let Some(prev) = ema_disp { prev * 0.85 + mid * 0.15 } else { mid };
            ema_disp = Some(smoothed);

            let alpha = 0.28f32;
            let beta  = 0.06f32;
            let dt    = 0.5f32;
            if trk_x.is_none() { trk_x = Some(smoothed); trk_v = 0.0; }
            if let Some(mut x) = trk_x {
                let x_pred = x + trk_v * dt;
                let z = smoothed;
                let gain_scale = if conf < 0.70 { 0.6 } else if conf < 0.80 { 0.8 } else { 1.0 };
                let a = alpha * gain_scale;
                let b = beta  * gain_scale;
                let r = z - x_pred;
                x = x_pred + a * r;
                trk_v = trk_v + (b * r) / dt;
                trk_x = Some(x);
                if conf < 0.80 { ema_disp = Some(x); }
            }

            if let Some(cell) = CURRENT_BPM.get() {
                if let Ok(mut guard) = cell.lock() {
                    let mut allow_hard = conf >= 0.80;
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
                    let soft_thr = if in_fast { 0.50 } else { 0.55 };
                    let allow_soft = if !allow_hard && conf >= soft_thr {
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

                    if allow_hard || allow_soft || allow_major {
                        let mut show_state = if allow_soft { "uncertain" } else { state };
                        let disp_int = disp.round() as i32;
                        if allow_soft {
                            if let Some(prev_int) = last_hard_int { if prev_int == disp_int { show_state = last_hard_state.unwrap_or("tracking"); } }
                            if in_fast {
                                if let Some(prev) = *guard { if prev.bpm.round() as i32 != disp_int { show_state = "uncertain"; } }
                            }
                        }
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
                                    emit_friendly(&app, format!("依据多数候选切换整数至 {} BPM", k), format!("Switched to majority integer {} BPM", k));
                                    DisplayBpm { bpm: k as f32, confidence: conf, state: "uncertain", level }
                                }
                                else { DisplayBpm { bpm: disp, confidence: conf, state: show_state, level } }
                            } else { DisplayBpm { bpm: disp, confidence: conf, state: show_state, level } }
                        };
                        *guard = Some(payload);
                        let _ = app.emit_to("main", "bpm_update", payload);
                        if !allow_soft {
                            let changed = match last_hard_int { Some(prev) => prev != disp_int, None => true };
                            if changed { emit_friendly(&app, format!("当前节拍：{} BPM", disp_int), format!("Current tempo: {} BPM", disp_int)); }
                            last_hard_int = Some(disp_int); last_hard_state = Some(state);
                        }
                        let nowh = now_ms();
                        recent_ints.push_back((payload.bpm.round() as i32, nowh));
                        while let Some(&(_, t0)) = recent_ints.front() { if nowh.saturating_sub(t0) > 1500 { recent_ints.pop_front(); } else { break; } }
                    }
                }
            }
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
            for _ in 0..hop_len { let _ = window.pop_front(); }
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
            if EMIT_TEXT_LOGS {
                let txt = if is_log_zh() { format!("[无结果] 本步无估计 电平={:.2} 追踪={} 连续空帧={}", level, tracking, none_cnt) } else { format!("[NONE] win step no estimate, lvl={:.2} tracking={} none_cnt={}", level, tracking, none_cnt) };
                eprintln!("{}", txt);
                let log = BackendLog { t_ms: now_ms(), msg: txt.clone() };
                let _ = app.emit_to("main", "bpm_log", log.clone());
                if let Some(cell) = COLLECTED_LOGS.get() { if let Ok(mut g) = cell.lock() { g.push(log); } }
            }
            emit_friendly(&app, "暂未检测到清晰节拍，继续聆听…", "No clear beat yet. Listening…");
            for _ in 0..hop_len { let _ = window.pop_front(); }
            if none_cnt >= 6 { recent_none_flag = true; }
        }
    }
}


