## Tauri IPC 与前端（MVP）

### 行为
- 应用启动即开始系统默认回放设备的环回捕获与 BPM 估计（无开始/停止按钮）

### 命令（前端 -> Rust）
- （无必须命令）
- 可选：保留 `stop_capture()` 作为调试用途（UI 不暴露）

### 事件（Rust -> 前端）
- `bpm_update { bpm: f32, confidence: f32 }`（必需，200–500 ms）
- `audio_chunk { samples: Float32Array }`（可选，仅在启用波形时发送，30–60 FPS）

### 前端组件（最小集合）
- `BPMDisplay`：超大号数字显示当前 BPM 与置信度指示
- `WaveformCanvas`（可选）：简化 Canvas 2D 波形绘制

### 性能与负载
- 优先保证 `bpm_update` 的及时性；如启用波形，限制每帧样本点数
- 后端做降采样，前端只绘制

### 指示
- 顶部状态指示：初始化 / 采集中 / 启动失败
