#![cfg(target_os = "macos")]

// 基于 ScreenCaptureKit 的系统音频捕获占位骨架：
// 先以“构建通过”为目标，后续填充 SCStream / SCContentFilter / SCAudioConfiguration 的真实实现。

use anyhow::{anyhow, Result};
use crossbeam_channel::{bounded, Receiver};
use once_cell::sync::Lazy;
use std::ffi::c_void;
use crossbeam_channel::Sender;

// 简易权限状态缓存：避免频繁调用
static PERM_OK: Lazy<std::sync::Mutex<Option<bool>>> = Lazy::new(|| std::sync::Mutex::new(None));

#[cfg(target_os = "macos")]
extern "C" {
    fn CGPreflightScreenCaptureAccess() -> bool;
    fn CGRequestScreenCaptureAccess() -> bool;
}

#[cfg(target_os = "macos")]
fn check_screen_recording_permission() -> bool {
    unsafe {
        if CGPreflightScreenCaptureAccess() { true } else { CGRequestScreenCaptureAccess() }
    }
}

pub struct AudioService {
    sample_rate: u32,
}

impl AudioService {
    pub fn start_loopback() -> Result<(Self, Receiver<Vec<f32>>, Receiver<u32>)> {
        let (frames_tx, frames_rx) = bounded::<Vec<f32>>(16);
        let (sr_tx, sr_rx) = bounded::<u32>(4);
        // 预置采样率（稍后以实际回调更新）
        let _ = sr_tx.try_send(48000);

        // 权限预检：若未授权，直接保持静音回退
        let perm = {
            let mut g = PERM_OK.lock().unwrap();
            if let Some(v) = *g { v } else { let v = check_screen_recording_permission(); *g = Some(v); v }
        };
        if !perm {
            std::thread::spawn(move || loop {
                std::thread::sleep(std::time::Duration::from_millis(100));
                let _ = frames_tx.try_send(Vec::new());
            });
            return Ok((Self { sample_rate: 48000 }, frames_rx, sr_rx));
        }

        // 初始化 ScreenCaptureKit 的仅音频流（Objective-C 桥接）
        unsafe extern "C" {
            fn sck_audio_start(tx_ptr: *mut std::ffi::c_void) -> bool;
            fn sck_audio_stop();
        }

        // 将 Rust 端的 Sender 包装为裸指针，交给 OC 侧回调使用
        let tx_box = Box::new(frames_tx);
        let tx_ptr = Box::into_raw(tx_box) as *mut std::ffi::c_void;
        let ok = unsafe { sck_audio_start(tx_ptr) };
        if !ok {
            // 启动失败：静音回退
            std::thread::spawn(move || loop {
                std::thread::sleep(std::time::Duration::from_millis(50));
                let _ = unsafe { (&*(tx_ptr as *mut Sender<Vec<f32>>)).try_send(Vec::new()) };
            });
        }

        Ok((Self { sample_rate: 48000 }, frames_rx, sr_rx))
    }

    pub fn sample_rate(&self) -> u32 { self.sample_rate }
}

// 提供给 Objective-C 侧的发送函数，将 PCM f32 缓冲推入 Rust 端信道
#[no_mangle]
pub extern "C" fn rs_send_f32_buffer(tx_ptr: *mut c_void, data: *const f32, len: usize) -> bool {
    if tx_ptr.is_null() || data.is_null() || len == 0 { return false; }
    let sender: &Sender<Vec<f32>> = unsafe { &*(tx_ptr as *const Sender<Vec<f32>>) };
    let slice = unsafe { std::slice::from_raw_parts(data, len) };
    // 拷贝为 Vec<f32>
    let mut mono = Vec::with_capacity(len);
    mono.extend_from_slice(slice);
    sender.try_send(mono).is_ok()
}


