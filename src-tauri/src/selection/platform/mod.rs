//! 平台取词实现层

use std::sync::Arc;

use crate::selection::config::SharedConfig;
use crate::selection::engine::TextCapturer;

#[cfg(target_os = "macos")]
pub mod macos;

/// 构造当前平台的取词器
#[cfg(target_os = "macos")]
pub fn create_capturer(config: SharedConfig) -> Arc<dyn TextCapturer> {
    Arc::new(macos::MacCapturer::new(config))
}

#[cfg(not(target_os = "macos"))]
pub fn create_capturer(_config: SharedConfig) -> Arc<dyn TextCapturer> {
    // todo 其他平台实现
}

#[cfg(target_os = "macos")]
pub fn foreground_app_id() -> Option<String> {
    macos::foreground_app_id()
}

#[cfg(not(target_os = "macos"))]
pub fn foreground_app_id() -> Option<String> {
    // todo
}

#[cfg(target_os = "macos")]
pub fn is_accessibility_trusted(prompt: bool) -> bool {
    macos::is_accessibility_trusted(prompt)
}

#[cfg(not(target_os = "macos"))]
pub fn is_accessibility_trusted(prompt: bool) -> bool {
    true
}
