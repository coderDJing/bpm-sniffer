use std::{fs, path::{Path, PathBuf}};

fn logo_dir() -> PathBuf {
    // 构建脚本当前工作目录为 crate 根（src-tauri）
    PathBuf::from("../logo")
}

fn generate_icon_from_logo(dst: &Path) -> bool {
    let sizes = [16u32, 32, 48, 256];
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
    if icon_path.exists() { return; }
    if !generate_icon_from_logo(icon_path) {
        generate_placeholder_icon(icon_path);
    }
}

fn main() {
    ensure_icon();
    tauri_build::build();
}
