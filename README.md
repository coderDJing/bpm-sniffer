运行 aubio 后端（无需用户安装 aubio）

1) 我们将 aubio 所需的 DLL 作为资源打包，位于 `src-tauri/bin/windows/x64/`：
   - `aubio.dll`
   - `libsndfile-1.dll`
   - `fftw3f-3.dll`（若 aubio 构建时依赖 FFTW）

2) 构建带 aubio 的版本：
   - 开发：`cargo run --features aubio-backend --manifest-path src-tauri/Cargo.toml`
   - 打包：`cargo build --release --features aubio-backend --manifest-path src-tauri/Cargo.toml`

3) Tauri 在打包时会将上述 DLL 放入应用目录，用户无需安装运行库。

4) 若 aubio 初始化失败，程序自动回退到 Simple 后端。

cargo run --manifest-path src-tauri/Cargo.toml