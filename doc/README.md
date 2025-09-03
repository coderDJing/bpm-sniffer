## 项目总览（bpm-sniffer）

### 核心目标（MVP）
- **唯一必需功能**: 实时显示“当前系统 BPM”的数字。
- **可选增强**: 简单波形可视化（Canvas 2D），可开关。

### 平台与范围
- **首发**: Windows 10/11（系统声音环回捕获）
- **后续**: macOS 适配（单独里程碑，不在 MVP）

### 技术栈（精简）
- **容器**: Tauri
- **后端（Rust）**: 音频捕获、基础 DSP、实时 BPM 估计、事件推送
- **前端**: React + Vite（或任一轻量前端），`<BPMDisplay />` 为核心组件；`<WaveformCanvas />` 为可选
- **库建议**: miniaudio（loopback 捕获），rustfft/realfft（若采用谱法），tracing（日志）

### 性能与指标
- **延迟**: 捕获→BPM 更新 < 150 ms（UI 刷新 200–500 ms）
- **稳定性**: 设备更换/采样率变化自动重建（MVP 允许仅跟随系统默认设备）
- **精度**: 目标 60–200 BPM，常见音乐 ±2–3 BPM 以内
- **UI 简化**: 仅一个大数字显示 BPM；波形可选

### 目录建议
- `src-tauri/`：Rust 核心（`audio/`, `dsp/`, `bpm/`, `ipc/`）
- `src/`：前端（`components/BPMDisplay.tsx`, `components/WaveformCanvas.tsx` 可选）
- `doc/`：文档

### 里程碑（精简）
1) Windows 环回捕获 + 简化 BPM 估计 + 数字显示（MVP）
2) 可选：波形可视化（Canvas 2D）
3) 后续：macOS 适配

### 开发环境
- Windows: Rust 工具链 + Node.js；（miniaudio 无需额外系统 SDK）

### 决策点（MVP 相关）
- 捕获后端：优先 miniaudio（实现快），保留 WASAPI 接口抽象
- BPM 算法：优先谱流 + 自相关（简单鲁棒），参数范围 60–200
- 事件：仅 `bpm_update{ bpm, confidence }`；若启用波形，再下发 `audio_chunk{ samples }`

> 目标是尽快实现“一个稳定的 BPM 数字”，其它一切皆可选。
