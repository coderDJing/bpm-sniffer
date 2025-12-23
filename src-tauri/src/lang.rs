use std::sync::{
    atomic::{AtomicBool, Ordering},
    OnceLock,
};

// 全局日志语言标志：true=中文，false=英文（可多次更新）
pub static LOG_ZH: OnceLock<AtomicBool> = OnceLock::new();

pub fn set_log_lang_zh(is_zh: bool) {
    LOG_ZH
        .get_or_init(|| AtomicBool::new(false))
        .store(is_zh, Ordering::SeqCst);
}

pub fn is_log_zh() -> bool {
    LOG_ZH
        .get_or_init(|| AtomicBool::new(false))
        .load(Ordering::SeqCst)
}
