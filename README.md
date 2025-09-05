## BPM Sniffer

轻量节拍(BPM)探测器。基于 Tauri v2 + React 构建。

还在开发中，不要轻信现在的代码和release

### 开发

```bash
pnpm dev
```

### 打包

```bash
pnpm build
```

- Windows 会在 `src-tauri/target/release/bundle/nsis/` 生成安装包（推荐分发）。
- 也会在 `src-tauri/target/release/` 生成便携版 exe（依赖运行环境/工具链）。
 - macOS 需要在 `src-tauri/icons/` 添加 `icon.icns`（应用与托盘图标），并会在 `src-tauri/target/release/bundle/dmg/` 生成 dmg 安装包。

### macOS 支持

- 最低要求：macOS 12.3+（ScreenCaptureKit）。
- 首次运行使用系统音频捕获（ScreenCaptureKit）时，系统会请求“屏幕录制”权限，需允许后方可采集系统声音。
- 托盘图标会自动适配浅/深色菜单栏（模板图标）。