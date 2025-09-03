## 架构与数据流（MVP 精简）

### 模块
- `audio`：系统声音环回捕获（Windows: miniaudio），统一到 `f32/48k/mono`
- `dsp`：轻量预处理（整流/平滑/降采样）
- `bpm`：实时估计（谱流+自相关 或 包络+自相关），输出 `bpm, confidence`
- `ipc`：推送 `bpm_update`（波形为可选扩展）

### 线程
- 音频回调线程：仅写环形缓冲
- 工作线程：批量读帧，计算 BPM（200–500 ms 更新一次）

### 数据通路
1) 回调 → `RingBuffer<f32>`（48k/mono）
2) 工作线程读取窗口（约 8–12 s 滑动窗口，步长 0.5 s）
3) 计算特征 → 自相关 → 候选 BPM → 倍/半频消歧 → 平滑
4) 通过 IPC 推送 `bpm_update { bpm, confidence }`

### 错误与日志
- `thiserror` + `anyhow` 聚合错误
- `tracing` 记录回调滞后/丢帧、BPM 收敛时间

### 可选扩展
- 若启用波形，可在后端做降采样（如每 512 样本取一点）并推送 `audio_chunk`，UI Canvas 绘制即可

### 非目标（MVP 不做）
- 录音/WAV 导出、频谱可视化、设备列表管理、打包签名等
