# BPM Sniffer Website

官网使用 `Vue 3 + Vite + Vite SSG` 构建，发布到 GitHub Pages。

## Commands

```bash
pnpm install
pnpm build
pnpm preview
```

## Release Data

- `pnpm build` 会先执行 `scripts/fetch-release.mjs`，把最新稳定版信息写入 `src/generated/release.json`。
- 首屏下载按钮的静态 HTML 默认先指向 `https://github.com/coderDJing/bpm-sniffer/releases/latest`，避免站点未重建时把旧版安装包直链吐出去。
- 页面挂载后会再次请求 GitHub Release API，成功后把按钮更新为最新 `.exe` 直链，并刷新版本号与更新摘要。

## Deploy

- `.github/workflows/website.yml` 会在 `website/**` 变更后重建官网。
- 同一个工作流也会在 GitHub Release 发布时重建官网，确保静态首屏版本号跟上最新正式版。
