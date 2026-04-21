use serde::Serialize;
use std::sync::{Mutex, OnceLock};
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use tauri::menu::Menu;

// 共享给各模块的展示结构体
#[derive(Serialize, Clone, Copy)]
pub struct DisplayBpm {
    pub bpm: f32,
    pub confidence: f32,
    pub state: &'static str,
    pub level: f32,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureSource {
    System = 0,
    Microphone = 1,
}

impl CaptureSource {
    pub fn from_u8(value: u8) -> Self {
        match value {
            1 => Self::Microphone,
            _ => Self::System,
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "system" => Some(Self::System),
            "microphone" => Some(Self::Microphone),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Microphone => "microphone",
        }
    }

    pub fn zh_label(self) -> &'static str {
        match self {
            Self::System => "电脑内部声音",
            Self::Microphone => "麦克风收音",
        }
    }

    pub fn en_label(self) -> &'static str {
        match self {
            Self::System => "system audio",
            Self::Microphone => "microphone input",
        }
    }
}

// 托盘右键菜单句柄：主窗口态不显示“恢复窗口”，悬浮态显示
pub struct TrayContextMenu {
    pub normal: Menu<tauri::Wry>,
    pub floating: Menu<tauri::Wry>,
}

// 全局共享状态（OnceLock+Mutex/Atomic）
pub static CURRENT_BPM: OnceLock<Mutex<Option<DisplayBpm>>> = OnceLock::new();
pub static COLLECTED_LOGS: OnceLock<Mutex<Vec<BackendLog>>> = OnceLock::new();
pub static RESET_REQUESTED: OnceLock<AtomicBool> = OnceLock::new();
pub static CAPTURE_RUNNING: OnceLock<AtomicBool> = OnceLock::new();
pub static CAPTURE_SOURCE: OnceLock<AtomicU8> = OnceLock::new();

pub fn get_capture_source() -> CaptureSource {
    let value = CAPTURE_SOURCE
        .get_or_init(|| AtomicU8::new(CaptureSource::System as u8))
        .load(Ordering::SeqCst);
    CaptureSource::from_u8(value)
}

pub fn set_capture_source_value(source: CaptureSource) {
    CAPTURE_SOURCE
        .get_or_init(|| AtomicU8::new(CaptureSource::System as u8))
        .store(source as u8, Ordering::SeqCst);
}

// 可视化输出的下采样波形长度（与前端保持一致）
pub const OUT_LEN: usize = 192;
pub const TRAY_ID: &str = "main-tray";


