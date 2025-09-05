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