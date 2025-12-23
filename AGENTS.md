# Repository Guidelines

## Project Structure & Module Organization

- `src/`: React + TypeScript UI (Vite).
- `src-tauri/`: Tauri v2 Rust backend (audio capture, BPM engine, commands). `src-tauri/dist/` is the generated frontend bundle.
- `website/`: Public website (Vue 3 + `vite-ssg`). Builds to `website/dist/`.
- `scripts/`: build helpers (e.g., updater endpoint generation).
- `doc/`: design notes and algorithms (see `doc/算法原理与流程.md`).
- `logo/`, `screenshot/`: image assets.

## Build, Test, and Development Commands

Prereqs: Node `>=18`, `pnpm`, Rust toolchain (MSVC) on Windows, and Tauri build prerequisites.

Desktop app (repo root):

- `pnpm install`: install dependencies.
- `pnpm dev`: build UI into `src-tauri/dist/`, prepare updater endpoints, then run `tauri dev`.
- `pnpm build`: build NSIS installer + updater; outputs under `src-tauri/target/release/bundle/nsis/`.
- `pnpm web:build`: build UI only (outputs `src-tauri/dist/`).
- `pnpm preview`: preview the Vite build.

Website (run in `website/`):

- `pnpm install`
- `pnpm dev` / `pnpm build` / `pnpm preview` (`pnpm build` runs `scripts/fetch-release.mjs`, requires network).

Rust checks (optional):

- `cd src-tauri && cargo fmt && cargo clippy && cargo test`

## Coding Style & Naming Conventions

- TypeScript: keep `strict` typing on; prefer small, focused components and hooks.
- Naming: React components use `PascalCase.tsx` (e.g., `src/component/WaveViz.tsx`); helpers use `camelCase`.
- Rust: format with `cargo fmt`; use `snake_case` for modules/functions.
- Don’t edit or commit generated artifacts: `src-tauri/dist/`, `src-tauri/target/`, `website/dist/`, `node_modules/`.

## Testing Guidelines

- No JS test runner is configured currently. Validate via `pnpm dev` and a short manual pass (audio playing, refresh, visualization switch, theme/language toggles).
- Add Rust unit tests for pure logic where it’s low-cost; run `cargo test` in `src-tauri/`.

## Commit & Pull Request Guidelines

- Git history mostly uses short, descriptive Chinese subjects (often including version bumps like `更新版本号至 1.1.5…`). Follow that style; use Conventional Commits when it helps automation (e.g., `fix(ci): ...`).
- PRs: include a clear description, link related issues, add screenshots for UI changes, and note release impact (stable tags `vX.Y.Z` vs pre-release `vX.Y.Z-rc1`).

## Security & Configuration Tips

- CI release signing expects GitHub Secrets `TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` (see `.github/workflows/release.yml`). Avoid committing personal signing keys; use secrets for CI.
