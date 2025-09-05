use std::{fs, path::{Path, PathBuf}};

fn logo_dir() -> PathBuf {
    // 构建脚本当前工作目录为 crate 根（src-tauri）
    PathBuf::from("../logo")
}

fn generate_icon_from_logo(dst: &Path) -> bool {
    let sizes = [16u32, 20, 24, 32, 40, 48, 256];
    let base = logo_dir();
    let mut ico_dir = ico::IconDir::new(ico::ResourceType::Icon);
    let mut found_any = false;

    for sz in sizes {
        let png = base.join(format!("{sz}.png"));
        if !png.exists() { continue; }
        match image::open(&png) {
            Ok(dynimg) => {
                let img = dynimg.to_rgba8();
                if img.width() == sz && img.height() == sz {
                    let entry = ico::IconImage::from_rgba_data(sz, sz, img.into_raw());
                    if let Ok(e) = ico::IconDirEntry::encode(&entry) {
                        ico_dir.add_entry(e);
                        found_any = true;
                    }
                }
            }
            Err(_) => {}
        }
    }

    if !found_any { return false; }

    if let Some(parent) = dst.parent() { let _ = fs::create_dir_all(parent); }
    let mut bytes = Vec::new();
    if ico_dir.write(&mut bytes).is_ok() {
        if fs::write(dst, bytes).is_ok() { return true; }
    }
    false
}

fn generate_placeholder_icon(dst: &Path) {
    if let Some(parent) = dst.parent() { let _ = fs::create_dir_all(parent); }
    let size = 256u32;
    let mut img = image::RgbaImage::new(size, size);
    for p in img.pixels_mut() { *p = image::Rgba([0x1a, 0x26, 0x33, 0xff]); }
    let dyn_img = image::DynamicImage::ImageRgba8(img);
    let mut ico = ico::IconDir::new(ico::ResourceType::Icon);
    let entry = ico::IconImage::from_rgba_data(size, size, dyn_img.to_rgba8().into_raw());
    if let Ok(e) = ico::IconDirEntry::encode(&entry) { ico.add_entry(e); }
    let mut bytes = Vec::new();
    if ico.write(&mut bytes).is_ok() { let _ = fs::write(dst, bytes); }
}

fn ensure_icon() {
    // 注意：这里必须是 crate 根下的路径：icons/icon.ico
    let icon_path = Path::new("icons/icon.ico");

    // 如果 ico 不存在，直接生成
    if !icon_path.exists() {
        if !generate_icon_from_logo(icon_path) {
            generate_placeholder_icon(icon_path);
        }
        return;
    }

    // 若任一源 PNG 比 ico 新，则重建 ico
    let srcs = [16u32, 20, 24, 32, 40, 48, 256]
        .into_iter()
        .map(|sz| logo_dir().join(format!("{sz}.png")))
        .collect::<Vec<_>>();

    let ico_mtime = fs::metadata(icon_path)
        .and_then(|m| m.modified())
        .ok();

    let mut need_regen = false;
    for p in &srcs {
        if !p.exists() { continue; }
        let newer = fs::metadata(p)
            .and_then(|m| m.modified())
            .ok()
            .map(|t| ico_mtime.map_or(true, |it| t > it))
            .unwrap_or(false);
        if newer { need_regen = true; break; }
    }

    if need_regen {
        if !generate_icon_from_logo(icon_path) {
            generate_placeholder_icon(icon_path);
        }
    }
}

#[cfg(target_os = "macos")]
fn build_macos_objc_bridge() {
    // 仅在 macOS 主机上编译并链接 Objective-C 桥接与系统框架
    println!("cargo:rerun-if-changed=src/macos_sck_audio.m");
    cc::Build::new()
        .file("src/macos_sck_audio.m")
        .flag("-fobjc-arc")
        .compile("macos_sck_audio");
    println!("cargo:rustc-link-lib=framework=ScreenCaptureKit");
    println!("cargo:rustc-link-lib=framework=AVFoundation");
    println!("cargo:rustc-link-lib=framework=CoreMedia");
    println!("cargo:rustc-link-lib=framework=CoreGraphics");
    println!("cargo:rustc-link-lib=framework=CoreAudio");
}

#[cfg(not(target_os = "macos"))]
fn build_macos_objc_bridge() {
    // 非 macOS 主机上不需要任何处理
}

fn main() {
    // 当任意来源 PNG 发生变更时，触发重新运行 build.rs
    for sz in [16u32, 20, 24, 32, 40, 48, 256] {
        println!("cargo:rerun-if-changed=../logo/{sz}.png");
    }
    // 按主机系统决定是否编译 macOS 桥接与链接框架
    build_macos_objc_bridge();
    ensure_icon();
    tauri_build::build();
}
