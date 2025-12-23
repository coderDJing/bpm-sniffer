use std::cmp::Ordering;
use std::collections::VecDeque;

use rustfft::{num_complex::Complex, FftPlanner};

const MAJOR_KEYS: [&str; 12] = [
    "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
];
const MINOR_KEYS: [&str; 12] = [
    "Cm", "C#m", "Dm", "D#m", "Em", "Fm", "F#m", "Gm", "G#m", "Am", "A#m", "Bm",
];
pub const NA_KEY: &str = "N/A(无调性)";

// Camelot Wheel（和声混音）编号：A=小调，B=大调
const MAJOR_CAMELOT: [&str; 12] = [
    "8B", "3B", "10B", "5B", "12B", "7B", "2B", "9B", "4B", "11B", "6B", "1B",
];
const MINOR_CAMELOT: [&str; 12] = [
    "5A", "12A", "7A", "2A", "9A", "4A", "11A", "6A", "1A", "8A", "3A", "10A",
];

pub fn camelot_from_key_name(key: &str) -> Option<&'static str> {
    for i in 0..12 {
        if key == MAJOR_KEYS[i] {
            return Some(MAJOR_CAMELOT[i]);
        }
        if key == MINOR_KEYS[i] {
            return Some(MINOR_CAMELOT[i]);
        }
    }
    None
}

fn camelot_number(key: &str) -> Option<u8> {
    let c = camelot_from_key_name(key)?;
    let mut num: u8 = 0;
    for b in c.bytes() {
        if b.is_ascii_digit() {
            num = num.saturating_mul(10) + (b - b'0');
        } else {
            break;
        }
    }
    if num == 0 { None } else { Some(num) }
}

fn same_camelot_number(a: &str, b: &str) -> bool {
    camelot_number(a).zip(camelot_number(b)).map_or(false, |(x, y)| x == y)
}

#[derive(Clone, Copy, Debug)]
pub struct KeyEstimate {
    pub key: &'static str,
    pub top2: &'static str,
    pub score: f32,
    pub score2: f32,
    pub gap: f32,
    pub confidence: f32,
    pub state: &'static str, // analyzing | atonal | uncertain | tracking
    pub effective_sec: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct KeyDecision {
    pub key: &'static str,
    pub state: &'static str, // tracking | uncertain | analyzing | atonal
    pub raw: KeyEstimate,
}

#[derive(Clone, Copy, Debug)]
pub struct KeyTrackerConfig {
    pub candidate_conf_min: f32,
    pub init_conf_min: f32,
    pub init_gap_min: f32,
    pub init_wins: usize,
    pub switch_conf_min: f32,
    pub switch_gap_min: f32,
    pub switch_wins: usize,
    pub mode_switch_conf_min: f32,
    pub mode_switch_gap_min: f32,
    pub mode_switch_wins: usize,
    pub hold_ms: u64,
}

impl Default for KeyTrackerConfig {
    fn default() -> Self {
        Self {
            candidate_conf_min: 0.35,
            init_conf_min: 0.45,
            init_gap_min: 0.05,
            init_wins: 3,
            switch_conf_min: 0.50,
            switch_gap_min: 0.06,
            switch_wins: 3,
            mode_switch_conf_min: 0.60,
            mode_switch_gap_min: 0.10,
            mode_switch_wins: 4,
            hold_ms: 4000,
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct StableKey {
    key: &'static str,
    last_confirm_ms: u64,
}

#[derive(Clone, Copy, Debug)]
struct PendingKey {
    key: &'static str,
    wins: usize,
}

pub struct KeyTracker {
    cfg: KeyTrackerConfig,
    stable: Option<StableKey>,
    pending: Option<PendingKey>,
}

impl KeyTracker {
    pub fn new() -> Self {
        Self::with_config(KeyTrackerConfig::default())
    }

    pub fn with_config(cfg: KeyTrackerConfig) -> Self {
        Self {
            cfg,
            stable: None,
            pending: None,
        }
    }

    pub fn reset(&mut self) {
        self.stable = None;
        self.pending = None;
    }

    pub fn update(&mut self, raw: KeyEstimate, now_ms: u64) -> KeyDecision {
        if raw.key == NA_KEY || raw.state == "atonal" {
            return self.hold_or_na(raw, now_ms, "atonal");
        }
        if raw.state == "analyzing" {
            return self.hold_or_na(raw, now_ms, "analyzing");
        }

        let can_count = raw.confidence >= self.cfg.candidate_conf_min;
        if can_count {
            match self.pending {
                Some(mut p) if p.key == raw.key => {
                    p.wins += 1;
                    self.pending = Some(p);
                }
                _ => {
                    self.pending = Some(PendingKey {
                        key: raw.key,
                        wins: 1,
                    });
                }
            }
        } else {
            self.pending = None;
        }

        if let Some(stable) = &mut self.stable {
            if raw.key == stable.key {
                stable.last_confirm_ms = now_ms;
                self.pending = None;
                let state = if raw.state == "tracking" {
                    "tracking"
                } else {
                    "uncertain"
                };
                return KeyDecision {
                    key: stable.key,
                    state,
                    raw,
                };
            }

            let same_num = same_camelot_number(stable.key, raw.key);
            let wins = self
                .pending
                .as_ref()
                .filter(|p| p.key == raw.key)
                .map(|p| p.wins)
                .unwrap_or(0);
            let (need_wins, conf_min, gap_min) = if same_num {
                (
                    self.cfg.mode_switch_wins,
                    self.cfg.mode_switch_conf_min,
                    self.cfg.mode_switch_gap_min,
                )
            } else {
                (
                    self.cfg.switch_wins,
                    self.cfg.switch_conf_min,
                    self.cfg.switch_gap_min,
                )
            };
            let can_switch =
                can_count && raw.confidence >= conf_min && raw.gap >= gap_min && wins >= need_wins;
            if can_switch {
                stable.key = raw.key;
                stable.last_confirm_ms = now_ms;
                self.pending = None;
                let state = if raw.state == "tracking" {
                    "tracking"
                } else {
                    "uncertain"
                };
                return KeyDecision {
                    key: stable.key,
                    state,
                    raw,
                };
            }

            let age = now_ms.saturating_sub(stable.last_confirm_ms);
            if age <= self.cfg.hold_ms {
                return KeyDecision {
                    key: stable.key,
                    state: "uncertain",
                    raw,
                };
            }

            self.stable = None;
            return KeyDecision {
                key: NA_KEY,
                state: "atonal",
                raw,
            };
        }

        let wins = self
            .pending
            .as_ref()
            .filter(|p| p.key == raw.key)
            .map(|p| p.wins)
            .unwrap_or(0);
        let can_lock = can_count
            && raw.confidence >= self.cfg.init_conf_min
            && raw.gap >= self.cfg.init_gap_min
            && wins >= self.cfg.init_wins;
        if can_lock {
            self.stable = Some(StableKey {
                key: raw.key,
                last_confirm_ms: now_ms,
            });
            let state = if raw.state == "tracking" {
                "tracking"
            } else {
                "uncertain"
            };
            return KeyDecision {
                key: raw.key,
                state,
                raw,
            };
        }

        KeyDecision {
            key: raw.key,
            state: raw.state,
            raw,
        }
    }

    fn hold_or_na(&mut self, raw: KeyEstimate, now_ms: u64, fall_state: &'static str) -> KeyDecision {
        if let Some(stable) = &mut self.stable {
            let age = now_ms.saturating_sub(stable.last_confirm_ms);
            if age <= self.cfg.hold_ms {
                self.pending = None;
                return KeyDecision {
                    key: stable.key,
                    state: "uncertain",
                    raw,
                };
            }
            self.stable = None;
        }
        self.pending = None;
        KeyDecision {
            key: NA_KEY,
            state: fall_state,
            raw,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum KeyProfile {
    // 经典 K-S profile，偏“古典/流行统计”，用于对照与回归
    #[allow(dead_code)]
    KrumhanslSchmuckler,
    // 面向电子音乐的启发式 profile：强化主三和弦与自然小调特征（可持续迭代调参）
    EdmTriadV1,
}

#[derive(Clone, Copy, Debug)]
pub struct KeyConfig {
    pub fft_size: usize,
    pub hop_size: usize,
    pub min_hz: f32,
    pub max_hz: f32,
    pub key_profile: KeyProfile,
    // 低频（bass）chroma 混合：增强主音线索（对 EDM/强低频曲目更友好）
    pub bass_max_hz: f32,
    pub bass_mix: f32, // 0..1
    pub bass_tonic_bonus: f32,
    pub mag_gamma: f32,
    pub ema_window_sec: f32,
    // tonalness 需要更快响应（区别于 key 的长窗平滑）
    pub tonal_window_sec: f32,
    // 模式（大小调）判别：根据 3 度能量给轻微偏置
    pub mode_third_bonus: f32,
    pub warmup_sec: f32,
    pub update_interval_ms: u64,
    pub min_level: f32,
    // Tonalness gate：低于阈值时输出 N/A(无调性)
    pub tonal_min: f32,
    pub tau_low: f32,
    pub tau_high: f32,
    pub reset_on_silence_ms: u64,
    pub silence_decay_sec: f32,
    // HPSS（Harmonic-Percussive Source Separation）参数：用于削弱打击乐对调性估计的干扰
    pub hpss_time_frames: usize,
    pub hpss_freq_bins: usize,
    pub hpss_mask_power: f32,
}

impl Default for KeyConfig {
    fn default() -> Self {
        Self {
            fft_size: 4096,
            hop_size: 2048,
            min_hz: 80.0,
            max_hz: 5000.0,
            key_profile: KeyProfile::EdmTriadV1,
            bass_max_hz: 250.0,
            bass_mix: 0.30,
            bass_tonic_bonus: 0.12,
            mag_gamma: 0.5,
            ema_window_sec: 12.0,
            tonal_window_sec: 2.0,
            mode_third_bonus: 0.06,
            warmup_sec: 4.0,
            update_interval_ms: 500,
            min_level: 0.03,
            tonal_min: 0.50,
            tau_low: 0.35,
            tau_high: 0.55,
            reset_on_silence_ms: 1500,
            silence_decay_sec: 2.0,
            hpss_time_frames: 9,
            hpss_freq_bins: 17,
            hpss_mask_power: 2.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum KeyMode {
    Major,
    Minor,
}

#[derive(Clone, Copy, Debug)]
struct KeyCandidate {
    mode: KeyMode,
    tonic: usize,
    score: f32,
}

#[derive(Clone, Copy, Debug)]
struct BinPitchContrib {
    k: usize,
    pc0: usize,
    w0: f32,
    pc1: usize,
    w1: f32,
    chroma_w: f32,
    is_bass: bool,
}

pub struct KeyEngine {
    cfg: KeyConfig,
    sample_rate: u32,
    stft: Stft,
    chroma_ema: [f32; 12],
    bass_ema: [f32; 12],
    tonal_ema: f32,
    processed_samples: u64,
    silence_ms: u64,
    last_update_ms: u64,
    major_tmpl: [f32; 12],
    minor_tmpl: [f32; 12],
    minor_harm_tmpl: [f32; 12],
}

impl KeyEngine {
    pub fn new(sample_rate: u32) -> Self {
        Self::with_config(sample_rate, KeyConfig::default())
    }

    pub fn with_config(sample_rate: u32, cfg: KeyConfig) -> Self {
        let (major_tmpl, minor_tmpl, minor_harm_tmpl) = build_templates(cfg.key_profile);
        Self {
            cfg,
            sample_rate,
            stft: Stft::new(sample_rate, cfg),
            chroma_ema: [0.0; 12],
            bass_ema: [0.0; 12],
            tonal_ema: 0.0,
            processed_samples: 0,
            silence_ms: 0,
            last_update_ms: 0,
            major_tmpl,
            minor_tmpl,
            minor_harm_tmpl,
        }
    }

    pub fn reset(&mut self, sample_rate: u32) {
        if self.sample_rate != sample_rate {
            self.sample_rate = sample_rate;
            self.stft = Stft::new(sample_rate, self.cfg);
        } else {
            self.stft.clear();
        }
        self.chroma_ema = [0.0; 12];
        self.bass_ema = [0.0; 12];
        self.tonal_ema = 0.0;
        self.processed_samples = 0;
        self.silence_ms = 0;
        self.last_update_ms = 0;
    }

    pub fn process(&mut self, samples: &[f32], level: f32, now_ms: u64) -> Option<KeyEstimate> {
        if samples.is_empty() {
            return None;
        }

        let dt_sec = samples.len() as f32 / (self.sample_rate.max(1) as f32);
        let dt_ms = (dt_sec * 1000.0).round().max(0.0) as u64;

        if level < self.cfg.min_level {
            self.silence_ms = self.silence_ms.saturating_add(dt_ms);
            self.apply_silence_decay(dt_sec);
            if self.silence_ms >= self.cfg.reset_on_silence_ms {
                self.reset(self.sample_rate);
            }
            return None;
        }
        self.silence_ms = 0;

        self.stft.push(samples);
        let hop_dt_sec = self.cfg.hop_size as f32 / (self.sample_rate.max(1) as f32);
        let alpha = if self.cfg.ema_window_sec > 0.0 {
            (hop_dt_sec / self.cfg.ema_window_sec).clamp(0.0, 1.0)
        } else {
            1.0
        };
        let alpha_tonal = if self.cfg.tonal_window_sec > 0.0 {
            (hop_dt_sec / self.cfg.tonal_window_sec).clamp(0.0, 1.0)
        } else {
            1.0
        };

        while let Some(frame) = self.stft.next_chroma(self.cfg.mag_gamma) {
            let (chroma_peak, chroma_peak2) = top_two_peaks(&frame.chroma);
            let (bass_peak, bass_peak2) = top_two_peaks(&frame.bass);
            let pitch_peak = chroma_peak.max(bass_peak);
            let pitch_peak2 = chroma_peak2.max(bass_peak2);

            // tonalness: 基于 HPSS 后“谐波谱”的谱平坦度、chroma 集中度，并结合谐波能量占比
            let tonal0 = (1.0 - frame.flatness).clamp(0.0, 1.0);
            // 对“无旋律/无稳定音高结构”的段落更严格：需要一定的 pitch 峰值/次峰值与谐波占比
            let tonal1a = ((pitch_peak - 0.18) / 0.18).clamp(0.0, 1.0);
            let tonal1b = ((pitch_peak2 - 0.10) / 0.12).clamp(0.0, 1.0);
            let tonal1 = (0.70 * tonal1a + 0.30 * tonal1b).clamp(0.0, 1.0);
            let tonal2 = ((frame.harmonic_ratio - 0.35) / 0.25).clamp(0.0, 1.0);
            // 防止仅靠“非平坦谱”误判（例如滤波噪声/打击）——提高 pitch 证据权重
            let tonal = (0.25 * tonal0 + 0.50 * tonal1 + 0.25 * tonal2).clamp(0.0, 1.0);

            self.tonal_ema = self.tonal_ema * (1.0 - alpha_tonal) + tonal * alpha_tonal;

            // 低 tonalness 时降低更新权重，避免无调性段稀释 chroma_ema
            let a = (alpha * tonal).clamp(0.0, 1.0);
            for i in 0..12 {
                self.chroma_ema[i] = self.chroma_ema[i] * (1.0 - a) + frame.chroma[i] * a;
                self.bass_ema[i] = self.bass_ema[i] * (1.0 - a) + frame.bass[i] * a;
            }
            self.processed_samples = self
                .processed_samples
                .saturating_add(self.cfg.hop_size as u64);
        }

        if now_ms.saturating_sub(self.last_update_ms) < self.cfg.update_interval_ms {
            return None;
        }
        self.last_update_ms = now_ms;

        let effective_sec = self.processed_samples as f32 / (self.sample_rate.max(1) as f32);
        let state = if effective_sec < self.cfg.warmup_sec {
            "analyzing"
        } else if self.tonal_ema < self.cfg.tonal_min {
            "atonal"
        } else {
            let chroma = normalize_l1(self.chroma_ema)?;
            let bass = normalize_l1(self.bass_ema);
            let bass_hint = bass.map(|b| bass_hint_from_chroma(b));
            let (best, second) = best_two_candidates_with_hint(
                &chroma,
                &self.major_tmpl,
                &self.minor_tmpl,
                &self.minor_harm_tmpl,
                bass_hint,
                self.cfg.bass_tonic_bonus,
                self.cfg.mode_third_bonus,
            );

            let score = best.score;
            let score2 = second.score;
            let gap = (score - score2).max(0.0);
            let score_n = (score / 0.70).clamp(0.0, 1.0);
            let gap_n = (gap / 0.15).clamp(0.0, 1.0);
            let confidence = (0.35 * score_n + 0.65 * gap_n).clamp(0.0, 1.0);

            let state = if confidence >= self.cfg.tau_high {
                "tracking"
            } else if confidence >= self.cfg.tau_low {
                "uncertain"
            } else {
                "analyzing"
            };

            return Some(KeyEstimate {
                key: candidate_name(best),
                top2: candidate_name(second),
                score,
                score2,
                gap,
                confidence,
                state,
                effective_sec,
            });
        };

        Some(KeyEstimate {
            key: NA_KEY,
            top2: NA_KEY,
            score: 0.0,
            score2: 0.0,
            gap: 0.0,
            confidence: 0.0,
            state,
            effective_sec,
        })
    }

    fn apply_silence_decay(&mut self, dt_sec: f32) {
        if self.cfg.silence_decay_sec <= 0.0 {
            return;
        }
        let k = (-dt_sec / self.cfg.silence_decay_sec).exp();
        for v in &mut self.chroma_ema {
            *v *= k;
        }
        for v in &mut self.bass_ema {
            *v *= k;
        }
        self.tonal_ema *= k;
    }
}

#[derive(Clone, Copy, Debug)]
struct ChromaFrame {
    chroma: [f32; 12],
    bass: [f32; 12],
    flatness: f32,
    harmonic_ratio: f32,
}

struct Stft {
    n: usize,
    hop: usize,
    window: Vec<f32>,
    bins: Vec<BinPitchContrib>,
    fft: std::sync::Arc<dyn rustfft::Fft<f32>>,
    buf: VecDeque<f32>,
    fft_buf: Vec<Complex<f32>>,
    // HPSS 状态（以选定频段的线性谱为输入）
    hpss_time: usize,
    hpss_freq: usize,
    hpss_power: f32,
    hpss_ring: Vec<Vec<f32>>,
    hpss_pos: usize,
    hpss_len: usize,
    mag_bins: Vec<f32>,
    h_med: Vec<f32>,
    p_med: Vec<f32>,
    median_scratch: Vec<f32>,
    bass_mix: f32,
}

impl Stft {
    fn new(sample_rate: u32, cfg: KeyConfig) -> Self {
        let n = cfg.fft_size.max(16);
        let hop = cfg.hop_size.clamp(1, n);
        let window = hann_window(n);
        let bins = build_bin_contribs(sample_rate, n, cfg.min_hz, cfg.max_hz, cfg.bass_max_hz);
        let num_bins = bins.len();
        let hpss_time = (cfg.hpss_time_frames.max(1) | 1).min(63);
        let hpss_freq = (cfg.hpss_freq_bins.max(1) | 1).min(63);
        let hpss_power = if cfg.hpss_mask_power.is_finite() && cfg.hpss_mask_power > 0.0 {
            cfg.hpss_mask_power
        } else {
            2.0
        };
        let mut hpss_ring = Vec::with_capacity(hpss_time);
        for _ in 0..hpss_time {
            hpss_ring.push(vec![0.0f32; num_bins]);
        }
        let mut planner = FftPlanner::<f32>::new();
        let fft = planner.plan_fft_forward(n);
        Self {
            n,
            hop,
            window,
            bins,
            fft,
            buf: VecDeque::with_capacity(n * 2),
            fft_buf: vec![Complex { re: 0.0, im: 0.0 }; n],
            hpss_time,
            hpss_freq,
            hpss_power,
            hpss_ring,
            hpss_pos: 0,
            hpss_len: 0,
            mag_bins: vec![0.0f32; num_bins],
            h_med: vec![0.0f32; num_bins],
            p_med: vec![0.0f32; num_bins],
            median_scratch: Vec::with_capacity(hpss_time.max(hpss_freq)),
            bass_mix: cfg.bass_mix.clamp(0.0, 1.0),
        }
    }

    fn clear(&mut self) {
        self.buf.clear();
        self.hpss_pos = 0;
        self.hpss_len = 0;
        for f in &mut self.hpss_ring {
            f.fill(0.0);
        }
    }

    fn push(&mut self, samples: &[f32]) {
        for &s in samples {
            self.buf.push_back(s);
        }
    }

    fn next_chroma(&mut self, mag_gamma: f32) -> Option<ChromaFrame> {
        if self.buf.len() < self.n {
            return None;
        }

        for i in 0..self.n {
            let s = *self.buf.get(i).unwrap_or(&0.0);
            self.fft_buf[i].re = s * self.window[i];
            self.fft_buf[i].im = 0.0;
        }
        self.fft.process(&mut self.fft_buf);

        let mut chroma_full = [0.0f32; 12];
        let mut chroma_bass = [0.0f32; 12];
        let eps = 1e-12f32;

        // 1) 构建当前帧幅度谱（仅频段内 bins）
        for (i, c) in self.bins.iter().enumerate() {
            let z = self.fft_buf[c.k];
            let mag_raw = (z.re * z.re + z.im * z.im).sqrt();
            self.mag_bins[i] = if mag_raw.is_finite() { mag_raw } else { 0.0 };
        }

        // 2) HPSS：时间中值（谐波）与频率中值（打击）估计
        if !self.hpss_ring.is_empty() {
            let slot = &mut self.hpss_ring[self.hpss_pos];
            slot.copy_from_slice(&self.mag_bins);
            self.hpss_pos = (self.hpss_pos + 1) % self.hpss_time;
            self.hpss_len = (self.hpss_len + 1).min(self.hpss_time);
        }

        let num_bins = self.mag_bins.len();
        let start = if self.hpss_len > 0 {
            (self.hpss_pos + self.hpss_time - self.hpss_len) % self.hpss_time
        } else {
            0
        };
        for i in 0..num_bins {
            self.median_scratch.clear();
            for t in 0..self.hpss_len {
                let idx = (start + t) % self.hpss_time;
                self.median_scratch.push(self.hpss_ring[idx][i]);
            }
            self.h_med[i] = median_unstable(&mut self.median_scratch);
        }

        let radius = self.hpss_freq / 2;
        for i in 0..num_bins {
            let s = i.saturating_sub(radius);
            let e = (i + radius + 1).min(num_bins);
            self.median_scratch.clear();
            self.median_scratch.extend_from_slice(&self.mag_bins[s..e]);
            self.p_med[i] = median_unstable(&mut self.median_scratch);
        }

        // 3) soft mask 提取谐波谱，再投影到 chroma
        let mut sum_total = 0.0f32;
        let mut sum_h = 0.0f32;
        let mut sum_log_h = 0.0f32;
        let mut cnt = 0usize;
        for (i, c) in self.bins.iter().enumerate() {
            let s = self.mag_bins[i];
            sum_total += s;
            let h = self.h_med[i].max(0.0);
            let p = self.p_med[i].max(0.0);
            let hp = h.powf(self.hpss_power);
            let pp = p.powf(self.hpss_power);
            let mh = if hp + pp > 0.0 {
                hp / (hp + pp + eps)
            } else {
                0.0
            };
            let mag_h = mh * s;
            sum_h += mag_h;
            sum_log_h += (mag_h + eps).ln();
            cnt += 1;

            let mut mag = mag_h;
            if mag_gamma > 0.0 && mag_gamma != 1.0 {
                mag = mag.powf(mag_gamma);
            }
            let m0 = mag * c.w0 * c.chroma_w;
            let m1 = mag * c.w1 * c.chroma_w;
            chroma_full[c.pc0] += m0;
            chroma_full[c.pc1] += m1;
            if c.is_bass {
                chroma_bass[c.pc0] += m0;
                chroma_bass[c.pc1] += m1;
            }
        }
        normalize_l1_in_place(&mut chroma_full);
        normalize_l1_in_place(&mut chroma_bass);

        let mut chroma = [0.0f32; 12];
        if self.bass_mix > 0.0 {
            for i in 0..12 {
                chroma[i] = chroma_full[i] * (1.0 - self.bass_mix) + chroma_bass[i] * self.bass_mix;
            }
        } else {
            chroma = chroma_full;
        }
        normalize_l1_in_place(&mut chroma);

        let flatness = if cnt > 0 {
            let am = (sum_h / (cnt as f32)).max(eps);
            let gm = (sum_log_h / (cnt as f32)).exp();
            (gm / am).clamp(0.0, 1.0)
        } else {
            1.0
        };
        let harmonic_ratio = if sum_total > eps {
            (sum_h / sum_total).clamp(0.0, 1.0)
        } else {
            0.0
        };

        for _ in 0..self.hop {
            let _ = self.buf.pop_front();
        }
        Some(ChromaFrame {
            chroma,
            bass: chroma_bass,
            flatness,
            harmonic_ratio,
        })
    }
}

fn median_unstable(values: &mut [f32]) -> f32 {
    let n = values.len();
    if n == 0 {
        return 0.0;
    }
    let mid = n / 2;
    values.select_nth_unstable_by(mid, |a, b| a.partial_cmp(b).unwrap_or(Ordering::Equal));
    let m = values[mid];
    if n % 2 == 1 {
        m
    } else {
        let mut max_lo = values[0];
        for &v in &values[..mid] {
            if v > max_lo {
                max_lo = v;
            }
        }
        0.5 * (max_lo + m)
    }
}

fn hann_window(n: usize) -> Vec<f32> {
    let mut w = vec![0.0f32; n];
    if n <= 1 {
        return w;
    }
    let denom = (n - 1) as f32;
    for i in 0..n {
        let x = 2.0 * std::f32::consts::PI * (i as f32) / denom;
        w[i] = 0.5 * (1.0 - x.cos());
    }
    w
}

fn build_bin_contribs(
    sample_rate: u32,
    n: usize,
    min_hz: f32,
    max_hz: f32,
    bass_max_hz: f32,
) -> Vec<BinPitchContrib> {
    let sr = sample_rate.max(1) as f32;
    let half = n / 2;
    let k_min = ((min_hz.max(1.0) * n as f32 / sr).ceil() as usize).clamp(1, half);
    let k_max =
        ((max_hz.max(min_hz).min(sr * 0.49) * n as f32 / sr).floor() as usize).clamp(1, half);
    let mut out = Vec::with_capacity(k_max.saturating_sub(k_min) + 1);
    for k in k_min..=k_max {
        let f = k as f32 * sr / n as f32;
        if f <= 0.0 {
            continue;
        }
        let semitone = 69.0 + 12.0 * (f / 440.0).log2();
        let s0 = semitone.floor();
        let frac = (semitone - s0).clamp(0.0, 1.0);
        let i0 = s0 as i32;
        let pc0 = i0.rem_euclid(12) as usize;
        let pc1 = (i0 + 1).rem_euclid(12) as usize;
        let chroma_w = (min_hz.max(1.0) / f).sqrt().clamp(0.0, 1.0);
        out.push(BinPitchContrib {
            k,
            pc0,
            w0: 1.0 - frac,
            pc1,
            w1: frac,
            chroma_w,
            is_bass: f <= bass_max_hz,
        });
    }
    out
}

fn normalize_l1(v: [f32; 12]) -> Option<[f32; 12]> {
    let sum: f32 = v.iter().sum();
    if sum <= 1e-9 {
        return None;
    }
    let mut out = [0.0f32; 12];
    for i in 0..12 {
        out[i] = v[i] / sum;
    }
    Some(out)
}

fn normalize_l1_in_place(v: &mut [f32; 12]) {
    let sum: f32 = v.iter().sum();
    if sum <= 1e-9 {
        v.fill(0.0);
        return;
    }
    for x in v {
        *x /= sum;
    }
}

fn top_two_peaks(v: &[f32; 12]) -> (f32, f32) {
    let mut best = 0.0f32;
    let mut second = 0.0f32;
    for &x in v {
        if x > best {
            second = best;
            best = x;
        } else if x > second {
            second = x;
        }
    }
    (best, second)
}

fn build_templates(profile: KeyProfile) -> ([f32; 12], [f32; 12], [f32; 12]) {
    match profile {
        KeyProfile::KrumhanslSchmuckler => {
            // Krumhansl–Schmuckler key profiles（未归一化）
            let major = [
                6.35, 2.23, 3.48, 2.33, 4.38, 4.09, 2.52, 5.19, 2.39, 3.66, 2.29, 2.88,
            ];
            let minor = [
                6.33, 2.68, 3.52, 5.38, 2.60, 3.53, 2.54, 4.75, 3.98, 2.69, 3.34, 3.17,
            ];
            let mut minor_harm = minor;
            minor_harm[10] *= 0.55;
            minor_harm[11] *= 1.35;
            (
                normalize_template(major),
                normalize_template(minor),
                normalize_template(minor_harm),
            )
        }
        KeyProfile::EdmTriadV1 => {
            // EDM 启发式 profile：
            // - 强调主三和弦（1/3/5）与属音（5）
            // - 小调强调自然小调 b6/b7（EDM 常见）
            // - 其余半音作为弱权重噪声底
            let major = [
                1.00, 0.15, 0.55, 0.15, 0.85, 0.60, 0.15, 0.90, 0.15, 0.50, 0.15, 0.45,
            ];
            let minor = [
                1.00, 0.15, 0.50, 0.85, 0.25, 0.55, 0.15, 0.90, 0.65, 0.15, 0.65, 0.30,
            ];
            let minor_harm = [
                1.00, 0.15, 0.50, 0.85, 0.25, 0.55, 0.15, 0.90, 0.65, 0.15, 0.20, 0.70,
            ];
            (
                normalize_template(major),
                normalize_template(minor),
                normalize_template(minor_harm),
            )
        }
    }
}

fn normalize_template(v: [f32; 12]) -> [f32; 12] {
    let mut sum = 0.0f32;
    for x in v {
        sum += x * x;
    }
    let norm = sum.sqrt().max(1e-9);
    let mut out = [0.0f32; 12];
    for i in 0..12 {
        out[i] = v[i] / norm;
    }
    out
}

fn best_two_candidates_with_hint(
    chroma: &[f32; 12],
    major: &[f32; 12],
    minor: &[f32; 12],
    minor_harm: &[f32; 12],
    bass_hint: Option<BassHint>,
    bass_tonic_bonus: f32,
    mode_third_bonus: f32,
) -> (KeyCandidate, KeyCandidate) {
    let mut best = KeyCandidate {
        mode: KeyMode::Major,
        tonic: 0,
        score: -1.0,
    };
    let mut second = best;
    for tonic in 0..12 {
        let bonus = bass_bonus(tonic, bass_hint, bass_tonic_bonus);
        let (major_bias, minor_bias) = mode_third_bias(chroma, tonic, mode_third_bonus);
        let s = rotated_corr(chroma, major, tonic) + bonus + major_bias;
        update_best2(
            KeyCandidate {
                mode: KeyMode::Major,
                tonic,
                score: s,
            },
            &mut best,
            &mut second,
        );
        let s_nat = rotated_corr(chroma, minor, tonic);
        let s_harm = rotated_corr(chroma, minor_harm, tonic);
        let s = s_nat.max(s_harm) + bonus + minor_bias;
        update_best2(
            KeyCandidate {
                mode: KeyMode::Minor,
                tonic,
                score: s,
            },
            &mut best,
            &mut second,
        );
    }
    (best, second)
}

fn mode_third_bias(chroma: &[f32; 12], tonic: usize, bonus: f32) -> (f32, f32) {
    if bonus <= 0.0 {
        return (0.0, 0.0);
    }
    let m3 = chroma[(tonic + 3) % 12];
    let m3 = if m3.is_finite() { m3 } else { 0.0 };
    let maj3 = chroma[(tonic + 4) % 12];
    let maj3 = if maj3.is_finite() { maj3 } else { 0.0 };
    let sum = m3 + maj3;
    if sum <= 1e-6 {
        return (0.0, 0.0);
    }
    let diff = (maj3 - m3) / sum;
    let major_bias = (diff.max(0.0) * bonus).clamp(0.0, bonus);
    let minor_bias = ((-diff).max(0.0) * bonus).clamp(0.0, bonus);
    (major_bias, minor_bias)
}
#[derive(Clone, Copy, Debug)]
struct BassHint {
    tonic: usize,
    strength: f32, // 0..1
}

fn bass_hint_from_chroma(bass: [f32; 12]) -> BassHint {
    let mut tonic = 0usize;
    let mut peak = bass[0];
    for i in 1..12 {
        if bass[i] > peak {
            peak = bass[i];
            tonic = i;
        }
    }
    // 经验：均匀分布时 peak≈1/12≈0.083；明显主音时 peak 通常 >0.20
    let strength = ((peak - 0.12) / 0.23).clamp(0.0, 1.0);
    BassHint { tonic, strength }
}

fn bass_bonus(tonic: usize, hint: Option<BassHint>, bonus: f32) -> f32 {
    let Some(h) = hint else { return 0.0 };
    if h.strength <= 0.0 || bonus <= 0.0 {
        return 0.0;
    }
    if tonic % 12 == h.tonic % 12 {
        bonus * h.strength
    } else {
        0.0
    }
}

// Pearson correlation (mean-centered) between chroma and rotated template.
// This avoids the "all-positive cosine similarity" degeneracy (uniform chroma can score very high for all keys).
fn rotated_corr(chroma: &[f32; 12], tmpl: &[f32; 12], tonic: usize) -> f32 {
    let mean_x = chroma.iter().sum::<f32>() / 12.0;
    let mean_t = tmpl.iter().sum::<f32>() / 12.0;
    let mut num = 0.0f32;
    let mut den_x = 0.0f32;
    let mut den_t = 0.0f32;
    for pc in 0..12 {
        let idx = (pc + 12 - (tonic % 12)) % 12;
        let x = chroma[pc] - mean_x;
        let t = tmpl[idx] - mean_t;
        num += x * t;
        den_x += x * x;
        den_t += t * t;
    }
    if den_x <= 1e-9 || den_t <= 1e-9 {
        0.0
    } else {
        num / (den_x * den_t).sqrt()
    }
}

fn update_best2(c: KeyCandidate, best: &mut KeyCandidate, second: &mut KeyCandidate) {
    if c.score > best.score {
        *second = *best;
        *best = c;
    } else if c.score > second.score {
        *second = c;
    }
}

fn candidate_name(c: KeyCandidate) -> &'static str {
    match c.mode {
        KeyMode::Major => MAJOR_KEYS[c.tonic % 12],
        KeyMode::Minor => MINOR_KEYS[c.tonic % 12],
    }
}
