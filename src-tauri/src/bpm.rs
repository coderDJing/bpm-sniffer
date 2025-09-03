use std::collections::VecDeque;
use serde::Serialize;

// 诊断日志开关：临时开启，便于定位 NONE 的原因（完成后可改为 false）
const VERBOSE_BPM_DEBUG: bool = false;

#[derive(Serialize, Clone, Copy)]
pub struct BpmEstimate { pub bpm: f32, pub confidence: f32, pub rms: f32, pub from_short: bool, pub win_sec: f32 }

pub struct BpmEstimator {
    sample_rate: u32,
    // 处理为较低速率以降低计算成本（例如 150 Hz）
    ds_rate: f32,
    // 缓存下采样后的包络序列（秒级窗口）
    buf: VecDeque<f32>,
    // 参数
    min_bpm: f32,
    max_bpm: f32,
    // 鼓点增强滤波状态（一阶高通→一阶低通，强调 40–180Hz）
    hp_alpha: f32,
    lp_alpha: f32,
    hp_lp_prev: f32,
    lp_prev: f32,
    prev_bp: f32,
    // 自适应：输入RMS（dBFS）用于检测突变
    last_input_rms_db: f32,
    // 短窗优先一致性计数
    last_short_bpm: Option<f32>,
    short_consistency: u8,
    // 记录上次输出的 BPM，用于自适应短窗长度
    last_bpm: Option<f32>,
}

impl BpmEstimator {
    pub fn new(sample_rate: u32) -> Self {
        // 一阶 IIR 系数：alpha = exp(-2*pi*fc/fs)
        let fs = sample_rate as f32;
        let fc_hp = 40.0f32;
        let fc_lp = 180.0f32;
        let hp_alpha = (-2.0 * std::f32::consts::PI * fc_hp / fs).exp();
        let lp_alpha = (-2.0 * std::f32::consts::PI * fc_lp / fs).exp();
        let this = Self {
            sample_rate,
            ds_rate: 200.0,
            buf: VecDeque::with_capacity(8 * 200),
            min_bpm: 91.0,
            max_bpm: 180.0,
            hp_alpha,
            lp_alpha,
            hp_lp_prev: 0.0,
            lp_prev: 0.0,
            prev_bp: 0.0,
            last_input_rms_db: -120.0,
            last_short_bpm: None,
            short_consistency: 0,
            last_bpm: None,
        };
        eprintln!("[INIT] BpmEstimator sr={} ds_rate={}", sample_rate, 200.0f32);
        this
    }

    pub fn push_frames(&mut self, frames: &[f32]) -> Option<BpmEstimate> {
        // 输入RMS（用于重置滤波状态）
        if !frames.is_empty() {
            let mut sum = 0.0f32; for &s in frames { sum += s * s; }
            let rms = (sum / frames.len() as f32).sqrt();
            let db = 20.0 * (rms.max(1e-9)).log10();
            if db - self.last_input_rms_db > 6.0 {
                // 输入音量突增，重置滤波与部分历史，避免旧状态干扰
                self.hp_lp_prev = 0.0; self.lp_prev = 0.0; self.prev_bp = 0.0;
                let keep = (self.ds_rate as usize) * 1; // 保留 1 秒尾部
                while self.buf.len() > keep { self.buf.pop_front(); }
                eprintln!("[RST] input_db jump {:.1} -> {:.1}, reset filters & trim buffer", self.last_input_rms_db, db);
            }
            self.last_input_rms_db = db;
        }
        // 简单整流 + 低通平滑 + 下采样到 ds_rate
        let decim = (self.sample_rate as f32 / self.ds_rate).round().max(1.0) as usize;
        if decim == 0 { return None; }

        let mut acc = 0.0f32;
        let mut cnt = 0usize;
        for (i, &s) in frames.iter().enumerate() {
            // 鼓点增强：一阶高通（近似）
            let hp_lp = self.hp_alpha * self.hp_lp_prev + (1.0 - self.hp_alpha) * s;
            let hp = s - hp_lp; // 高通近似输出
            self.hp_lp_prev = hp_lp;
            // 一阶低通，限制至 180Hz
            let lp = self.lp_alpha * self.lp_prev + (1.0 - self.lp_alpha) * hp;
            self.lp_prev = lp;
            // 正向差分强调攻击
            let pd = (lp - self.prev_bp).max(0.0);
            self.prev_bp = lp;

            acc += pd;
            cnt += 1;
            if (i + 1) % decim == 0 {
                let m = acc / cnt as f32;
                // 一阶 IIR 平滑
                let prev = *self.buf.back().unwrap_or(&m);
                let smoothed = prev * 0.8 + m * 0.2;
                self.buf.push_back(smoothed);
                acc = 0.0; cnt = 0;
            }
        }

        // 维持 4 秒窗口（更快响应强节奏；精度由峰过滤与一致性保证）
        let max_len = (self.ds_rate as usize) * 4;
        while self.buf.len() > max_len { self.buf.pop_front(); }
        if self.buf.len() < (self.ds_rate as usize) * 1 { return None; }

        // 能量门限（静音/弱信号直接不给估计）
        let rms = (self.buf.iter().map(|v| v * v).sum::<f32>() / self.buf.len() as f32).sqrt();
        if rms < 4e-4 { eprintln!("[GATE] ds_rms={:.6} below threshold, skip", rms); return None; }

        // 公共参数
        let min_lag = (self.ds_rate * 60.0 / self.max_bpm).round() as usize;
        let max_lag = (self.ds_rate * 60.0 / self.min_bpm).round() as usize;

        // 计算器：对任意切片做自相关打分并返回 (bpm, confidence, best_score, avg_score)
        // 附加：仅在存在足够清晰的“鼓点峰”时才输出（低鼓点段落直接判定为 None）
        let eval_slice = |xs: &[f32]| -> Option<(f32, f32, f32, f32, f32)> {
            if xs.len() <= max_lag + 1 {
                if VERBOSE_BPM_DEBUG { eprintln!("[FILT] too_short xs_len={} need>{}", xs.len(), max_lag + 1); }
                return None;
            }
            // 首尾静音裁剪：阈值=3%峰值
            let max_v = xs.iter().cloned().fold(0.0f32, |m, v| if v.abs() > m { v.abs() } else { m });
            if max_v <= 1e-7 { if VERBOSE_BPM_DEBUG { eprintln!("[FILT] max_v too small max_v={:.3e}", max_v); } return None; }
            let thr = (0.03 * max_v).max(1e-7);
            let mut i0 = 0usize; while i0 < xs.len() && xs[i0].abs() < thr { i0 += 1; }
            let mut i1 = xs.len().saturating_sub(1); while i1 > i0 && xs[i1].abs() < thr { i1 -= 1; }
            if i1 <= i0 { if VERBOSE_BPM_DEBUG { eprintln!("[FILT] trim collapse i0={} i1={}", i0, i1); } return None; }
            let slice = &xs[i0..=i1];
            let min_keep = (self.ds_rate as usize) * 16 / 10; // 至少 1.6 秒
            if slice.len() < min_keep { if VERBOSE_BPM_DEBUG { eprintln!("[FILT] slice too short len={} need>={}", slice.len(), min_keep); } return None; }

            let mean = slice.iter().copied().sum::<f32>() / slice.len() as f32;
            let mut x: Vec<f32> = slice.iter().map(|v| v - mean).collect();
            // 汉宁窗，减弱边缘效应
            let n = x.len(); if n >= 3 {
                for i in 0..n {
                    let w = 0.5 * (1.0 - (2.0 * std::f32::consts::PI * i as f32 / (n as f32 - 1.0)).cos());
                    x[i] *= w as f32;
                }
            }

            // 鼓点峰提取：放宽基础阈值、动态阈值与显著性
            let max_x = x.iter().fold(0.0f32, |m, v| m.max(v.abs()));
            let peak_thr = (0.14 * max_x).max(1e-7);
            let mut abs_sorted: Vec<f32> = x.iter().map(|v| v.abs()).collect();
            abs_sorted.sort_by(|a,b| a.partial_cmp(b).unwrap());
            let p35 = abs_sorted[abs_sorted.len() * 35 / 100].max(1e-7);
            let dyn_thr = peak_thr.max(p35);
            let min_sep = ((self.ds_rate * 60.0 / self.max_bpm).round() as usize).max(2);
            let mut peaks: Vec<usize> = Vec::new();
            for i in 1..(x.len().saturating_sub(1)) {
                if x[i] > dyn_thr && x[i] >= x[i-1] && x[i] > x[i+1] {
                    if let Some(&last) = peaks.last() { if i.saturating_sub(last) < min_sep { continue; } }
                    let w = min_sep.max(4);
                    let l0 = i.saturating_sub(w);
                    let r0 = (i + w).min(x.len() - 1);
                    let left_min = x[l0..i].iter().fold(x[i], |m, &v| m.min(v));
                    let right_min = x[i+1..=r0].iter().fold(x[i], |m, &v| m.min(v));
                    let prom = x[i] - left_min.max(right_min);
                    let prom_thr = (0.04 * max_x).max(dyn_thr * 0.15);
                    if prom >= prom_thr { peaks.push(i); }
                }
            }
            if peaks.len() < 2 { if VERBOSE_BPM_DEBUG { eprintln!("[FILT] peaks<2 dyn_thr={:.4} peak_thr={:.4} max_x={:.4}", dyn_thr, peak_thr, max_x); } return None; }

            // 计算鼓点间隔的变异系数
            let mut cv = 0.0f32;
            if peaks.len() >= 4 {
                let iois: Vec<f32> = peaks.windows(2).map(|w| (w[1]-w[0]) as f32).collect();
                let mean = iois.iter().copied().sum::<f32>() / iois.len() as f32;
                if mean > 1e-3 {
                    let var = iois.iter().map(|v| (v-mean)*(v-mean)).sum::<f32>() / iois.len() as f32;
                    let std = var.sqrt(); cv = (std / mean).min(2.0);
                }
                if cv > 0.65 { if VERBOSE_BPM_DEBUG { eprintln!("[FILT] cv high cv={:.3} thr=0.65", cv); } return None; }
            }

            let win_sec_here = slice.len() as f32 / self.ds_rate;
            // 峰强度分布：峰态
            let mut abs_sorted_all: Vec<f32> = x.iter().map(|v| v.abs()).collect();
            abs_sorted_all.sort_by(|a,b| a.partial_cmp(b).unwrap());
            let p50_all = abs_sorted_all[abs_sorted_all.len()/2].max(1e-7);
            let p95_all = abs_sorted_all[abs_sorted_all.len()*95/100].max(1e-7);
            let peakiness = (p95_all / p50_all).max(0.0);
            if peakiness < 1.25 { if VERBOSE_BPM_DEBUG { eprintln!("[FILT] peakiness low p95/p50={:.3}", peakiness); } return None; }

            // 峰密度范围
            let peaks_per_sec = peaks.len() as f32 / win_sec_here.max(1e-3);
            if peaks_per_sec < 0.20 || peaks_per_sec > 14.0 { if VERBOSE_BPM_DEBUG { eprintln!("[FILT] density out pps={:.2}", peaks_per_sec); } return None; }

            // 峰数量：窗口越长要求越高（4s 目标至少 4 个峰）
            let min_peaks = if win_sec_here >= 3.5 { 4 } else if win_sec_here >= 2.0 { 2 } else { 2 };
            if peaks.len() < min_peaks { if VERBOSE_BPM_DEBUG { eprintln!("[FILT] not enough peaks cnt={} need>={} win={:.2}s", peaks.len(), min_peaks, win_sec_here); } return None; }

            // 基于鼓点裁剪：保留第一个至最后一个峰的区间，左右各留 0.75*median_ioi 的缓冲
            let (start_i, end_i) = {
                let iois_for_pad: Vec<usize> = peaks.windows(2).map(|w| w[1]-w[0]).collect();
                let mut pad = if iois_for_pad.is_empty() { ((self.ds_rate * 60.0 / self.max_bpm).round() as usize) / 2 } else { let mut s = iois_for_pad.clone(); s.sort(); (s[s.len()/2] as f32 * 0.75) as usize };
                if pad < 2 { pad = 2; }
                let s = peaks.first().copied().unwrap_or(0); let e = peaks.last().copied().unwrap_or(x.len().saturating_sub(1));
                (s.saturating_sub(pad), (e+pad).min(x.len().saturating_sub(1)))
            };
            let xw_crop: &[f32] = &x[start_i..=end_i];
            // 若峰对齐裁剪后不足长度，回退到未裁剪 slice，避免过裁剪导致直接 None
            let xw: &[f32] = if xw_crop.len() < min_keep {
                if slice.len() >= min_keep {
                    if VERBOSE_BPM_DEBUG { eprintln!("[FALLBACK] use un-cropped slice len={} instead of cropped len={}", slice.len(), xw_crop.len()); }
                    slice
                } else {
                    if VERBOSE_BPM_DEBUG { eprintln!("[FILT] xw too short after peak-crop len={} and raw slice len={} both < {}", xw_crop.len(), slice.len(), min_keep); }
                    return None;
                }
            } else { xw_crop };

            // 自相关（峰对齐窗口 xw 上）
            let mut best_lag = 0usize;
            let mut best_score = f32::MIN;
            let mut second_score = f32::MIN;
            let mut scores_sum = 0.0f32;
            let mut scores_cnt = 0usize;

            // 栅格先验参数：优先整数/0.5 栅格，温和加权
            let grid_sigma = 0.32f32; // BPM 栅格标准差
            let grid_gamma = 0.30f32; // 栅格权重系数（温和）
            for lag in min_lag..=max_lag {
                let mut num = 0.0f32;
                let mut e1 = 0.0f32;
                let mut e2 = 0.0f32;
                if xw.len() <= lag { break; }
                for i in 0..(xw.len() - lag) {
                    let a = xw[i];
                    let b = xw[i + lag];
                    num += a * b;
                    e1 += a * a;
                    e2 += b * b;
                }
                let denom = (e1 * e2).sqrt().max(1e-9);
                let r = (num / denom).clamp(0.0, 1.0);
                let bpm_here = 60.0 * self.ds_rate / lag as f32;
                // 全局先验（中心 120，σ=50）
                let prior = (-((bpm_here - 120.0).powi(2)) / (2.0 * 50.0f32.powi(2))).exp();
                // 栅格先验（仅整数优先，去掉半整数以避免 130.5 倾向）
                let nearest_int = bpm_here.round();
                let delta = (bpm_here - nearest_int).abs();
                let grid_w = (-(delta * delta) / (2.0 * grid_sigma * grid_sigma)).exp();
                let grid_mix = (1.0 - grid_gamma) + grid_gamma * grid_w;
                let score = r * (0.6 + 0.4 * prior) * grid_mix;

                scores_sum += score;
                scores_cnt += 1;
                if score > best_score { second_score = best_score; best_score = score; best_lag = lag; }
                else if score > second_score { second_score = score; }
            }
            if best_lag == 0 { if VERBOSE_BPM_DEBUG { eprintln!("[FILT] no best lag"); } return None; }

            // 多谐波一致性：对 best_lag 对应的 bpm 以及 0.5x/2x 做一致性检查
            let _bpm_primary = 60.0 * self.ds_rate / best_lag as f32;
            let lag_half = ((best_lag as f32) * 2.0).round() as usize; // 0.5x bpm → 2×lag
            let lag_dbl  = ((best_lag as f32) * 0.5).round() as usize; // 2x bpm → 0.5×lag
            let eval_r_at = |lag: usize| -> f32 {
                if lag <= min_lag || lag >= max_lag || xw.len() <= lag { return 0.0; }
                let mut num = 0.0f32; let mut e1 = 0.0f32; let mut e2 = 0.0f32;
                for i in 0..(xw.len() - lag) { let a = xw[i]; let b = xw[i + lag]; num += a * b; e1 += a * a; e2 += b * b; }
                let denom = (e1 * e2).sqrt().max(1e-9); (num / denom).clamp(0.0, 1.0)
            };
            let r_primary = eval_r_at(best_lag);
            let r_half = eval_r_at(lag_half);
            let r_dbl  = eval_r_at(lag_dbl);
            // 族一致性差：仅影响后续 confidence，不改变排序分数，避免 margin 被连带压小
            let family_min = r_half.min(r_dbl);
            let harm_penalty = if family_min < r_primary * 0.35 {
                if VERBOSE_BPM_DEBUG { eprintln!("[HARM] penalize r_pri={:.3} r_half={:.3} r_dbl={:.3}", r_primary, r_half, r_dbl); }
                0.90
            } else { 1.0 };

            // 守卫
            if best_lag <= min_lag + 1 && best_score < 0.20 { if VERBOSE_BPM_DEBUG { eprintln!("[FILT] edge lag score too low lag={} score={:.3}", best_lag, best_score); } return None; }
            if best_score < 0.08 { if VERBOSE_BPM_DEBUG { eprintln!("[FILT] best_score too low {:.3}", best_score); } return None; }

            let avg_score = (scores_sum / scores_cnt as f32).max(1e-9);
            let ratio = (best_score / avg_score).max(1.0);
            // 主峰与次峰区分度
            let margin = (best_score - second_score).max(0.0);
            if margin < 0.010 { if VERBOSE_BPM_DEBUG { eprintln!("[FILT] margin too small {:.4}", margin); } return None; }
            let mut confidence = (best_score * ratio.sqrt()).clamp(0.0, 0.95).powf(0.85) * harm_penalty;
            // 置信度调制
            let cnt_factor = ((peaks.len() as f32) / 8.0).clamp(0.3, 1.0);
            let stab_factor = (1.0 - cv).clamp(0.4, 1.0);
            confidence *= 0.5 + 0.5 * cnt_factor * stab_factor;
            if confidence < 0.05 { if VERBOSE_BPM_DEBUG { eprintln!("[FILT] confidence too low {:.3}", confidence); } return None; }

            // 抛物线插值细化
            let mut bpm = 60.0 * self.ds_rate / best_lag as f32;
            if best_lag > min_lag && best_lag < max_lag {
                let corr_at = |lag: usize| -> f32 {
                    let mut num = 0.0f32; let mut e1 = 0.0f32; let mut e2 = 0.0f32;
                    if xw.len() <= lag { return 0.0; }
                    for i in 0..(xw.len() - lag) { let a = xw[i]; let b = xw[i + lag]; num += a * b; e1 += a * a; e2 += b * b; }
                    let denom = (e1 * e2).sqrt().max(1e-9); (num / denom).clamp(0.0, 1.0)
                };
                let r_m1 = corr_at(best_lag - 1);
                let r_0  = corr_at(best_lag);
                let r_p1 = corr_at(best_lag + 1);
                let denom = r_m1 - 2.0 * r_0 + r_p1;
                if denom.abs() > 1e-6 {
                    let d = 0.5 * (r_m1 - r_p1) / denom;
                    let d = d.clamp(-0.5, 0.5);
                    bpm = 60.0 * self.ds_rate / (best_lag as f32 + d);
                }
            }

            // IOI 中值二次确认：用鼓点间隔的中值得到 ioi_bpm，与自相关 bpm 接近则做温和加权
            if peaks.len() >= 3 {
                let mut iois_samp: Vec<usize> = peaks.windows(2).map(|w| w[1] - w[0]).collect();
                iois_samp.sort_unstable();
                let med_ioi = iois_samp[iois_samp.len()/2].max(1);
                let ioi_bpm = 60.0 * self.ds_rate / med_ioi as f32;
                let rel = (ioi_bpm - bpm).abs() / bpm.max(1e-6);
                if rel <= 0.008 { // 0.8% 以内则融合，降低0.5/1/2量化漂移
                    bpm = (bpm * 2.0 + ioi_bpm) / 3.0;
                }
            }
            let win_sec = xw.len() as f32 / self.ds_rate;
            if VERBOSE_BPM_DEBUG {
                eprintln!(
                    "[ANA-DBG] bpm={:.2} conf={:.3} win={:.2}s peaks={} pps={:.2} cv={:.3} margin={:.4} best={:.3} avg={:.3}",
                    bpm, confidence, win_sec, peaks.len(), peaks_per_sec, cv, margin, best_score, avg_score
                );
            }
            Some((bpm, confidence, best_score, avg_score, win_sec))
        };

        // 长窗（最多12s，实际为 buf 全部）
        let long_est = eval_slice(&self.buf.as_slices().0)
            .or_else(|| { let v: Vec<f32> = self.buf.iter().copied().collect(); eval_slice(&v) });

        // 自适应短窗：保证至少采到 >= 2–3 个鼓点周期
        let base_bpm = self.last_bpm.unwrap_or(140.0).clamp(self.min_bpm, self.max_bpm);
        let period_sec = 60.0 / base_bpm; // 单个鼓点周期
        let target_beats = 2.5f32; // 至少 2–3 个鼓点
        let short_sec = (target_beats * period_sec).clamp(2.0, 4.0); // 不低于2s，不高于4s
        let short_len = (self.ds_rate * short_sec).round() as usize;
        let short_est = if self.buf.len() > short_len {
            let start = self.buf.len() - short_len;
            let v: Vec<f32> = self.buf.iter().skip(start).copied().collect();
            eval_slice(&v)
        } else { None };

        // 短窗优先：差异>6%，短窗置信度≥长窗75%，且短窗连续两次一致
        let (bpm, confidence, from_short, win_sec) = match (long_est, short_est) {
            (Some((bl, cl, _, _, wl)), Some((bs, cs, _, _, ws))) => {
                let diverge = ((bs - bl).abs() / bl.max(1e-6)) > 0.06;
                // 短窗一致性计数
                if let Some(prev) = self.last_short_bpm { if (bs - prev).abs() / bs.max(1e-6) <= 0.03 { self.short_consistency = self.short_consistency.saturating_add(1); } else { self.short_consistency = 1; } } else { self.short_consistency = 1; }
                self.last_short_bpm = Some(bs);
                let prefer_short = diverge && cs >= cl * 0.75 && self.short_consistency >= 2;
                if prefer_short {
                    eprintln!("[SEL] short bs={:.1} cs={:.2} wl={:.1}s diverge={} sc={} vs long bl={:.1} cl={:.2}", bs, cs, ws, diverge, self.short_consistency, bl, cl);
                    (bs, cs, true, ws)
                } else {
                    eprintln!("[SEL] long  bl={:.1} cl={:.2} wl={:.1}s vs short bs={:.1} cs={:.2} diverge={} sc={}", bl, cl, wl, bs, cs, diverge, self.short_consistency);
                    (bl, cl, false, wl)
                }
            }
            (Some((bl, cl, _, _, wl)), None) => (bl, cl, false, wl),
            (None, Some((bs, cs, _, _, ws))) => (bs, cs, true, ws),
            (None, None) => return None,
        };

        // 记录上次输出 BPM
        self.last_bpm = Some(bpm);
        eprintln!("[OUT] bpm={:.1} conf={:.2} src={} win={:.1}s", bpm, confidence, if from_short { 'S' } else { 'L' }, win_sec);
        Some(BpmEstimate { bpm, confidence, rms, from_short, win_sec })
    }
}

// SimpleBackend 适配 TempoBackend 抽象
pub struct SimpleBackend { inner: BpmEstimator }
impl SimpleBackend {
    pub fn new(sample_rate: u32) -> Self { Self { inner: BpmEstimator::new(sample_rate) } }
    pub fn process_frames(&mut self, frames: &[f32]) -> Option<BpmEstimate> { self.inner.push_frames(frames) }
}
