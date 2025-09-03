## 按应用音频抓取（后续小里程碑）

### 目标
- 允许用户选择“某个正在发声的应用/进程”，仅嗅探其音频并进行 BPM 估计。

### 平台与前置
- **Windows**：需要 Windows 10 2004+，使用 Process Loopback
  - 关键点：`AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK` + `TargetProcessId`
  - 路径：`ActivateAudioInterfaceAsync` / `IAudioClient` / `IAudioSessionEnumerator`
  - 限制：被标记为 `ExcludeFromCapture` 的会话、DRM 保护流不可抓取
- **macOS**：建议 macOS 12.3+ 使用 ScreenCaptureKit（按应用/窗口 + 音频）
  - 关键点：`SCContentFilter` 选择 App/Window，启用 Audio Capture
  - 授权：系统会弹出录制权限窗口，需 UI 指引
  - 旧系统（<12.3）：需虚拟声卡路由，不作为首选

### UI 交互草案
1) 下拉列表显示“正在发声的会话/应用”（按音量峰值排序）
2) 用户选择目标应用后启动捕获；不满足条件时提示并降级为整机混音
3) 状态栏提示：当前抓取源（App 名称/进程名）

### 实现步骤（分平台）
- Windows
  - 枚举音频会话 → 关联 PID/进程名 → 过滤活跃会话
  - 使用 `Process Loopback` 打开目标 PID 的环回流
  - 版本检测与回退到整机混音
- macOS
  - 使用 ScreenCaptureKit 列出可选 App/Window
  - 创建 `SCStream`，启用音频并绑定所选 App
  - 权限失败或系统不支持时回退

### 风险与对策
- 权限/DRM/排除策略导致抓取失败 → 明确提示并自动回退
- 会话快速切换（App 静音/设备变化）→ 监听并重建流
- 性能：优先保证 BPM 更新；可视化为可选

### 里程碑交付
- Windows 首发支持（2004+），可选择任意活跃应用抓取
- macOS 12.3+ 跟进支持；旧系统不强行兼容

> 本功能不影响 MVP“整机混音→BPM 数字”的主路径，仅在系统支持且用户需要时启用。
