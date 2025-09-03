#![cfg(windows)]

use anyhow::{anyhow, Result};
use crossbeam_channel::{bounded, Receiver};
use std::{ptr, thread, time::Duration};
use windows::core::PCWSTR;
use windows::Win32::Media::Audio::*;
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CoTaskMemFree, CLSCTX_ALL, COINIT_MULTITHREADED};
use windows::Win32::System::Threading::{CreateEventW, WaitForSingleObject, INFINITE};
use windows::Win32::Foundation::CloseHandle;

const WAVE_FORMAT_PCM: u16 = 1;
const WAVE_FORMAT_IEEE_FLOAT: u16 = 3;

pub struct AudioService {
    sample_rate: u32,
}

impl AudioService {
    pub fn start_loopback() -> Result<(Self, Receiver<Vec<f32>>)> {
        let (frames_tx, frames_rx) = bounded::<Vec<f32>>(16);
        // 初始化结果通道：Ok(sample_rate) 或 Err
        let (init_tx, init_rx) = bounded::<Result<u32>>(1);

        thread::spawn(move || unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED).ok();

            // 设备枚举器（仅创建一次）
            let enumerator: IMMDeviceEnumerator = match CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) {
                Ok(v) => v,
                Err(e) => { let _ = init_tx.send(Err(anyhow!("{e:?}"))); return; }
            };

            // 首次初始化结果仅发送一次（用于返回采样率）
            let mut sent_init = false;

            loop {
                // 获取当前默认渲染端点并激活 AudioClient
                let device = match enumerator.GetDefaultAudioEndpoint(eRender, eConsole) {
                    Ok(v) => v,
                    Err(e) => {
                        if !sent_init { let _ = init_tx.send(Err(anyhow!("{e:?}"))); }
                        thread::sleep(Duration::from_millis(300));
                        continue;
                    }
                };
                let audio_client: IAudioClient = match device.Activate(CLSCTX_ALL, None) {
                    Ok(v) => v,
                    Err(e) => {
                        if !sent_init { let _ = init_tx.send(Err(anyhow!("{e:?}"))); }
                        thread::sleep(Duration::from_millis(300));
                        continue;
                    }
                };

                // 混音格式
                let pwfx = match audio_client.GetMixFormat() {
                    Ok(p) => p,
                    Err(e) => {
                        if !sent_init { let _ = init_tx.send(Err(anyhow!("{e:?}"))); }
                        thread::sleep(Duration::from_millis(300));
                        continue;
                    }
                };
                if pwfx.is_null() {
                    if !sent_init { let _ = init_tx.send(Err(anyhow!("GetMixFormat returned null"))); }
                    thread::sleep(Duration::from_millis(300));
                    continue;
                }
                let mix = &*pwfx;
                let sample_rate = mix.nSamplesPerSec;

                // 事件 & 初始化共享环回
                let h_event = match CreateEventW(None, false, false, PCWSTR::null()) {
                    Ok(h) => h,
                    Err(e) => {
                        if !sent_init { let _ = init_tx.send(Err(anyhow!("{e:?}"))); }
                        CoTaskMemFree(Some(pwfx as _));
                        thread::sleep(Duration::from_millis(300));
                        continue;
                    }
                };
                let buffer_duration = 200_000; // 20ms (100ns 单位)
                if let Err(e) = audio_client.Initialize(
                    AUDCLNT_SHAREMODE_SHARED,
                    AUDCLNT_STREAMFLAGS_LOOPBACK | AUDCLNT_STREAMFLAGS_EVENTCALLBACK,
                    buffer_duration,
                    0,
                    mix,
                    None,
                ) {
                    if !sent_init { let _ = init_tx.send(Err(anyhow!("{e:?}"))); }
                    let _ = CloseHandle(h_event);
                    CoTaskMemFree(Some(pwfx as _));
                    thread::sleep(Duration::from_millis(300));
                    continue;
                }
                let _ = audio_client.SetEventHandle(h_event);

                // 捕获客户端
                let capture_client: IAudioCaptureClient = match audio_client.GetService() {
                    Ok(c) => c,
                    Err(e) => {
                        if !sent_init { let _ = init_tx.send(Err(anyhow!("{e:?}"))); }
                        let _ = CloseHandle(h_event);
                        CoTaskMemFree(Some(pwfx as _));
                        thread::sleep(Duration::from_millis(300));
                        continue;
                    }
                };

                // 首次初始化时返回采样率
                if !sent_init { let _ = init_tx.send(Ok(sample_rate)); sent_init = true; }

                // 开始流并进入捕获循环；任何错误或检测到默认设备变更将导致跳出并重建
                let _ = audio_client.Start();
                loop {
                    // 检测默认设备是否改变（例如切换到蓝牙耳机）
                    let mut changed = false;
                    if let Ok(def) = enumerator.GetDefaultAudioEndpoint(eRender, eConsole) {
                        if let (Ok(id_now), Ok(id_cur)) = (def.GetId(), device.GetId()) {
                            let s_now = id_now.to_string().unwrap_or_default();
                            let s_cur = id_cur.to_string().unwrap_or_default();
                            if s_now != s_cur { changed = true; }
                            CoTaskMemFree(Some(id_now.0 as _));
                            CoTaskMemFree(Some(id_cur.0 as _));
                        }
                    }
                    if changed { break; }

                    let _ = WaitForSingleObject(h_event, INFINITE);
                    let mut packet_len = match capture_client.GetNextPacketSize() { Ok(n) => n, Err(_) => break };
                    while packet_len > 0 {
                        let mut data_ptr: *mut u8 = ptr::null_mut();
                        let mut num_frames: u32 = 0;
                        let mut flags_u32: u32 = 0;
                        let mut dev_pos: u64 = 0;
                        let mut qpc_pos: u64 = 0;

                        if capture_client.GetBuffer(
                            &mut data_ptr,
                            &mut num_frames,
                            &mut flags_u32,
                            Some((&mut dev_pos) as *mut u64),
                            Some((&mut qpc_pos) as *mut u64),
                        ).is_err() { break; }

                        if num_frames > 0 {
                            let channels = mix.nChannels as usize;
                            let total_samples = (num_frames as usize) * channels;
                            let mut mono = Vec::with_capacity(num_frames as usize);
                            let is_silent = (flags_u32 & AUDCLNT_BUFFERFLAGS_SILENT.0 as u32) != 0;
                            if !is_silent && !data_ptr.is_null() {
                                if mix.wFormatTag == WAVE_FORMAT_IEEE_FLOAT || mix.wBitsPerSample == 32 {
                                    let slice = std::slice::from_raw_parts(data_ptr as *const f32, total_samples);
                                    for frame in slice.chunks(channels) { let l = frame[0]; let r = *frame.get(1).unwrap_or(&l); mono.push((l + r) * 0.5); }
                                } else if mix.wFormatTag == WAVE_FORMAT_PCM && mix.wBitsPerSample == 16 {
                                    let slice = std::slice::from_raw_parts(data_ptr as *const i16, total_samples);
                                    for frame in slice.chunks(channels) {
                                        let l = frame[0] as f32 / 32768.0; let r = *frame.get(1).unwrap_or(&frame[0]) as f32 / 32768.0; mono.push((l + r) * 0.5);
                                    }
                                } else {
                                    mono.resize(num_frames as usize, 0.0);
                                }
                            } else {
                                mono.resize(num_frames as usize, 0.0);
                            }
                            let _ = frames_tx.try_send(mono);
                        }

                        let _ = capture_client.ReleaseBuffer(num_frames);
                        packet_len = match capture_client.GetNextPacketSize() { Ok(n) => n, Err(_) => 0 };
                    }
                }

                let _ = audio_client.Stop();
                let _ = CloseHandle(h_event);
                CoTaskMemFree(Some(pwfx as _));

                // 小憩后重试（处理默认设备切换、蓝牙断连、设备无效等）
                thread::sleep(Duration::from_millis(300));
            }
        });

        // 等待初始化结果（拿到采样率或错误）
        let sample_rate = init_rx.recv().map_err(|_| anyhow!("audio init channel closed"))??;
        Ok((Self { sample_rate }, frames_rx))
    }

    pub fn sample_rate(&self) -> u32 { self.sample_rate }
}
