## Windows 系统声音捕获（WASAPI Loopback）

### 方案选型
- **miniaudio**
  - 优点：API 简单、跨平台、Loopback 直接可用；后续迁移成本低
  - 风险：极端设备/编码边缘情况需要验证
- **WASAPI 原生**
  - 优点：可控性强，细节（共享/独占、格式协商）可精准把控
  - 成本：开发量较大，异步回调/缓冲管理更复杂
- **结论**：优先 **miniaudio** 完成首版，保留 WASAPI 原生实现接口以便回退/对齐

### 实现要点
- **设备选择**：默认选系统 `Default Playback` 的 Loopback 设备；提供枚举与切换
- **格式统一**：转为 `f32 / 48 kHz / mono`，必要时做重采样与混声道
- **缓冲策略**：回调写入环形缓冲（无锁环形队列/锁轻量），DSP 线程批量读取
- **设备变化**：监听默认设备变更/采样率变更，自动重建流
- **音量与静音**：Loopback 受系统输出音量影响（后混音），UI 提示；可提供输入增益
- **延迟目标**：端到端 < 150 ms；回调帧长 256–1024；DSP 更新 30–60 Hz

### 代码结构（建议）
- `src-tauri/src/audio/mod.rs`：公共接口、后端选择、环形缓冲
- `src-tauri/src/audio/windows.rs`：Windows 具体实现（miniaudio/WASAPI）
- `src-tauri/src/dsp/`：预处理/FFT
- `src-tauri/src/bpm/`：BPM 估计器

### 伪代码（miniaudio 思路）
```rust
struct AudioService { /* 省略 */ }

impl AudioService {
    fn start_loopback(&mut self, device_id: Option<String>) -> Result<()> {
        // 1) 选择默认回放设备的 loopback
        // 2) 打开数据回调，样本统一到 f32/48k/mono
        // 3) 在回调中写入 RingBuffer
        Ok(())
    }
}
```

### 测试与验证
- 带耳机/外放切换、不同采样率（44.1k/48k/96k）、系统音量调节
- 播放不同类型音乐（鼓点强/弱、电子/爵士/古典）验证 BPM 稳定性
- 压力测试：前端渲染开启/关闭对 CPU 的影响

### 已知限制
- 独占模式应用可能导致无法捕获（大部分流媒体/播放器为共享模式）
- 系统混音为音量后信号，用户音量会影响捕获电平
