#![cfg(windows)]

use anyhow::{anyhow, Result};
use crossbeam_channel::{bounded, Receiver};
use std::{ptr, thread, time::Duration};
use windows::core::PCWSTR;
use windows::Win32::Media::Audio::*;
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CoTaskMemFree, CLSCTX_ALL, COINIT_MULTITHREADED};
use windows::Win32::System::Threading::{CreateEventW, WaitForSingleObject};
use windows::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0, WAIT_TIMEOUT};

use crate::state::{get_capture_source, CaptureSource};
use crate::logging::append_log_line;

const WAVE_FORMAT_PCM: u16 = 1;
const WAVE_FORMAT_IEEE_FLOAT: u16 = 3;
// 44.1kHz 是现有算法已验证的输入基准；只给更高采样率设备限速，低采样率设备原样通过。
const MAX_ANALYSIS_SAMPLE_RATE: u32 = 44_100;

/// 跨 WASAPI 数据包保持状态的盒式低通降采样器。
/// 相位不能在每个数据包重置，否则 44.1kHz 等非整数比采样率会出现时间抖动。
struct AnalysisDownsampler {
    input_rate: u32,
    output_rate: u32,
    phase: u64,
    sum: f64,
    count: u32,
}

impl AnalysisDownsampler {
    fn new(input_rate: u32) -> Self {
        Self {
            input_rate,
            output_rate: input_rate.min(MAX_ANALYSIS_SAMPLE_RATE),
            phase: 0,
            sum: 0.0,
            count: 0,
        }
    }

    fn output_rate(&self) -> u32 { self.output_rate }

    fn process(&mut self, input: &[f32]) -> Vec<f32> {
        let mut output = Vec::with_capacity(
            input.len().saturating_mul(self.output_rate as usize) / self.input_rate as usize + 2,
        );
        for &sample in input {
            self.sum += sample as f64;
            self.count += 1;
            self.phase += self.output_rate as u64;
            if self.phase >= self.input_rate as u64 {
                output.push((self.sum / self.count as f64) as f32);
                self.phase -= self.input_rate as u64;
                self.sum = 0.0;
                self.count = 0;
            }
        }
        output
    }
}

#[cfg(test)]
mod tests {
    use super::AnalysisDownsampler;

    #[test]
    fn preserves_timing_across_arbitrary_packet_boundaries() {
        let input: Vec<f32> = (0..44_100).map(|i| i as f32 / 44_100.0).collect();

        let mut whole = AnalysisDownsampler::new(44_100);
        let expected = whole.process(&input);

        let mut packetized = AnalysisDownsampler::new(44_100);
        let mut actual = Vec::new();
        for packet in input.chunks(735) {
            actual.extend(packetized.process(packet));
        }

        assert_eq!(expected.len(), 44_100);
        assert_eq!(actual, expected);
    }

    #[test]
    fn caps_high_rate_input_at_the_algorithm_baseline() {
        let input = vec![0.25; 192_000];
        let mut downsampler = AnalysisDownsampler::new(192_000);

        let output = downsampler.process(&input);

        assert_eq!(downsampler.output_rate(), 44_100);
        assert_eq!(output.len(), 44_100);
    }
}

fn endpoint_flow(source: CaptureSource) -> EDataFlow {
    match source {
        CaptureSource::System => eRender,
        CaptureSource::Microphone => eCapture,
    }
}

fn stream_flags(source: CaptureSource) -> u32 {
    match source {
        CaptureSource::System => AUDCLNT_STREAMFLAGS_LOOPBACK | AUDCLNT_STREAMFLAGS_EVENTCALLBACK,
        CaptureSource::Microphone => AUDCLNT_STREAMFLAGS_EVENTCALLBACK,
    }
}

pub struct AudioService {
    sample_rate: u32,
}

impl AudioService {
    pub fn start_capture() -> Result<(Self, Receiver<Vec<f32>>, Receiver<u32>)> {
        let (frames_tx, frames_rx) = bounded::<Vec<f32>>(16);
        // 初始化结果通道：Ok(sample_rate) 或 Err
        let (init_tx, init_rx) = bounded::<Result<u32>>(1);
        // 运行时采样率变化通知通道（非阻塞，容量小即可）
        let (sr_tx, sr_rx) = bounded::<u32>(4);

        thread::spawn(move || unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED).ok();

            // 设备枚举器（仅创建一次）
            let enumerator: IMMDeviceEnumerator = match CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) {
                Ok(v) => v,
                Err(e) => { let _ = init_tx.send(Err(anyhow!("{e:?}"))); return; }
            };

            // 首次初始化结果仅发送一次（用于返回采样率）
            let mut sent_init = false;
            // 最近一次成功启动时的采样率，用于变化检测
            let mut last_sr: Option<u32> = None;
            let mut last_source: Option<CaptureSource> = None;

            loop {
                let source = get_capture_source();
                let data_flow = endpoint_flow(source);

                // 获取当前默认端点并激活 AudioClient
                let device = match enumerator.GetDefaultAudioEndpoint(data_flow, eConsole) {
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
                let mut downsampler = AnalysisDownsampler::new(sample_rate);
                let analysis_sample_rate = downsampler.output_rate();
                let mut dropped_packets = 0u64;
                let mut last_drop_log = std::time::Instant::now();

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
                    stream_flags(source),
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

                // 采样率变化通知（含首次启动）
                if last_sr.map_or(true, |prev| prev != sample_rate) || last_source.map_or(true, |prev| prev != source) {
                    let _ = sr_tx.try_send(analysis_sample_rate);
                    let message = format!(
                        "[AUDIO] input_rate={}Hz analysis_rate={}Hz source={}",
                        sample_rate,
                        analysis_sample_rate,
                        source.as_str(),
                    );
                    eprintln!("{}", message);
                    append_log_line(&message);
                    last_sr = Some(sample_rate);
                    last_source = Some(source);
                }
                // 首次初始化时返回采样率
                if !sent_init { let _ = init_tx.send(Ok(analysis_sample_rate)); sent_init = true; }

                // 开始流并进入捕获循环；任何错误或检测到默认设备变更将导致跳出并重建
                let _ = audio_client.Start();
                loop {
                    if get_capture_source() != source { break; }

                    // 检测默认设备是否改变（例如切换到蓝牙耳机或默认麦克风）
                    let mut changed = false;
                    if let Ok(def) = enumerator.GetDefaultAudioEndpoint(endpoint_flow(source), eConsole) {
                        if let (Ok(id_now), Ok(id_cur)) = (def.GetId(), device.GetId()) {
                            let s_now = id_now.to_string().unwrap_or_default();
                            let s_cur = id_cur.to_string().unwrap_or_default();
                            if s_now != s_cur { changed = true; }
                            CoTaskMemFree(Some(id_now.0 as _));
                            CoTaskMemFree(Some(id_cur.0 as _));
                        }
                    }
                    if changed { break; }

                    // 使用有限超时避免设备失效时的死等，超时则回到循环顶部做设备变更检查
                    let wait_res = WaitForSingleObject(h_event, 200);
                    if wait_res == WAIT_OBJECT_0 {
                        // 事件触发，继续读取缓冲
                    } else if wait_res == WAIT_TIMEOUT {
                        // 超时，回到顶部检查设备是否已切换
                        continue;
                    } else {
                        // 异常，跳出重建
                        break;
                    }
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
                            let analysis_frames = downsampler.process(&mono);
                            if !analysis_frames.is_empty() && frames_tx.try_send(analysis_frames).is_err() {
                                dropped_packets += 1;
                                if last_drop_log.elapsed() >= Duration::from_secs(1) {
                                    let message = format!(
                                        "[AUDIO] analysis queue overrun: dropped_packets={} input_rate={}Hz analysis_rate={}Hz",
                                        dropped_packets,
                                        sample_rate,
                                        analysis_sample_rate,
                                    );
                                    eprintln!("{}", message);
                                    append_log_line(&message);
                                    last_drop_log = std::time::Instant::now();
                                }
                            }
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
        Ok((Self { sample_rate }, frames_rx, sr_rx))
    }

    pub fn sample_rate(&self) -> u32 { self.sample_rate }
}
