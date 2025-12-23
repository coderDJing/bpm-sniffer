use serde::Serialize;
use std::sync::atomic::AtomicBool;
use std::sync::{Mutex, OnceLock};

// 共享给各模块的展示结构体
#[derive(Serialize, Clone, Copy)]
pub struct DisplayBpm {
    pub bpm: f32,
    pub confidence: f32,
    pub state: &'static str,
    pub level: f32,
}

#[derive(Serialize, Clone)]
pub struct DisplayKey {
    pub key: String,
    pub camelot: String,
    pub confidence: f32,
    pub state: &'static str,
}

#[derive(Serialize, Clone)]
pub struct BackendLog {
    pub t_ms: u64,
    pub msg: String,
}

#[derive(Serialize, Clone)]
pub struct AudioViz {
    // 下采样后的波形样本，范围约 [-1, 1]
    pub samples: Vec<f32>,
    // 当前包的 RMS（0-1）
    pub rms: f32,
}

// 全局共享状态（OnceLock+Mutex/Atomic）
pub static CURRENT_BPM: OnceLock<Mutex<Option<DisplayBpm>>> = OnceLock::new();
pub static COLLECTED_LOGS: OnceLock<Mutex<Vec<BackendLog>>> = OnceLock::new();
pub static RESET_REQUESTED: OnceLock<AtomicBool> = OnceLock::new();
pub static CAPTURE_RUNNING: OnceLock<AtomicBool> = OnceLock::new();

// 可视化输出的下采样波形长度（与前端保持一致）
pub const OUT_LEN: usize = 192;
