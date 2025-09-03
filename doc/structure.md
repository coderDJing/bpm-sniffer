## 目录结构与最佳实践（MVP）

### 当前结构（Vite + React）
- `src/`：前端源码（React + Vite）
- `dist/`：前端构建产物（Tauri 使用该目录）
- `src-tauri/`：Tauri 后端（Rust）、配置与打包
  - `Cargo.toml`、`src/main.rs`、`tauri.conf.json`
  - `icons/`：Windows 图标文件（请放置 `icon.ico`）
- `doc/`：文档

该结构符合 Tauri 官方常见实践，适合扩展 UI 与构建优化。

### 迁移说明
- 早期使用的 `public/` 静态目录已废弃；请以 `npm run build` 产出的 `dist/` 作为前端资源目录。
- `tauri.conf.json` 中 `build.frontendDist` 已指向 `../dist`。

### 建议
- 将 `src-tauri/gen/` 与 `src-tauri/target/` 排除在版本控制之外
- 图标路径：`src-tauri/icons/icon.ico`（建议包含 256×256）
