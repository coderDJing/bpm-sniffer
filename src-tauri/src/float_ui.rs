// 悬浮窗命中尺寸：与前端 FloatBall 的理论最大可视圈计算保持一致
// JS 对应：src/App.tsx -> FloatBall
pub fn float_canvas_size_logical() -> f64 {
    let ball_size = 58.0f64;
    let base_stroke = 1.68f64;
    let width_gain = 2.24f64;
    let radius_gain = 2.56f64;
    let hit_padding = 2.0f64;

    let r_base = (ball_size / 2.0 - base_stroke / 2.0 + radius_gain).max(8.0);
    let max_outer_radius = r_base + radius_gain;
    let max_stroke_width = base_stroke + width_gain;
    let max_viz_radius = max_outer_radius + max_stroke_width / 2.0;
    let raw = ((max_viz_radius + hit_padding) * 2.0).ceil();

    // 取偶数尺寸，保证中心点落在整数像素上，减少轻微锯齿偏移感
    if (raw as i32) % 2 == 0 { raw } else { raw + 1.0 }
}

