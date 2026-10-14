use serde::{Deserialize, Serialize};
use std::sync::{Arc, RwLock};

// 触发模式
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TriggerMode {
    // 选中自动翻译
    Auto,
    // 选中出翻译入口，点击翻译
    SemiAuto,
    // 快捷键翻译
    Hotkey,
}

// 抑制规则配置
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SuppressConfig {
    // 应用黑名单：进程名小写，如 ["keepass.exe"]
    pub app_blacklist: Vec<String>,
    // 应用白名单，(whitelist_mode = true 时生效)
    pub app_whitelist: Vec<String>,
    // 白名单模式，默认关
    pub whitelist_mode: bool,
    // 最小触发长度，默认 1
    pub min_text_length: usize,
    // 最大触发长度，默认 5000
    pub max_text_length: usize,
}

impl Default for SuppressConfig {
    fn default() -> Self {
        Self {
            app_blacklist: vec![
                "windowsterminal.exe".into(),
                "keepass.exe".into(), // 预设终端/密码管理器
            ],
            app_whitelist: Vec::new(),
            whitelist_mode: false,
            min_text_length: 1,
            max_text_length: 5000,
        }
    }
}

// 选词引擎配置
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SelectionConfig {
    // 是否开启划词翻译，默认开启
    pub enabled: bool,
    // 触发模式，默认自动
    pub trigger_mode: TriggerMode,
    // 选区稳定防抖时长，默认 300
    pub trigger_delay_ms: u32,
    // 拖拽判定阈值：位移 ≥ 此值才算拖拽选择，范围 5–10px，默认 8
    pub drag_threshold_px: u32,
    // 取词失败时是否允许剪贴板回退
    pub clipboard_fallback: bool,
    pub suppress: SuppressConfig,
}

impl Default for SelectionConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            trigger_mode: TriggerMode::Auto,
            trigger_delay_ms: 300,
            drag_threshold_px: 8,
            clipboard_fallback: true,
            suppress: SuppressConfig::default(),
        }
    }
}

impl SelectionConfig {
    pub fn normalized(mut self) -> Self {
        self.trigger_delay_ms = self.trigger_delay_ms.clamp(100, 1000);
        self.drag_threshold_px = self.drag_threshold_px.clamp(5, 10);
        self.suppress.min_text_length = self.suppress.min_text_length.max(1);
        self.suppress.max_text_length = self
            .suppress
            .max_text_length
            .max(self.suppress.min_text_length);
        self
    }
}

#[derive(Clone)]
pub struct SharedConfig(Arc<RwLock<SelectionConfig>>);

impl SharedConfig {
    pub fn new(config: SelectionConfig) -> Self {
        Self(Arc::new(RwLock::new(config.normalized())))
    }

    // 读配置
    pub fn get(&self) -> SelectionConfig {
        self.0.read().unwrap().clone()
    }

    // 热更新配置
    pub fn update(&self, config: SelectionConfig) -> SelectionConfig {
        let config = config.normalized();
        *self.0.write().unwrap() = config.clone();
        config
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_product_spec() {
        let c = SelectionConfig::default();
        assert_eq!(c.trigger_mode, TriggerMode::Auto);
        assert_eq!(c.trigger_delay_ms, 300);
        assert_eq!(c.drag_threshold_px, 8);
        assert_eq!(c.suppress.max_text_length, 5000)
    }

    #[test]
    fn normalizes_out_of_range_values() {
        let c = SelectionConfig {
            trigger_delay_ms: 5000,
            drag_threshold_px: 1,
            suppress: SuppressConfig {
                min_text_length: 0,
                max_text_length: 3,
                ..Default::default()
            },
            ..Default::default()
        }
        .normalized();
        assert_eq!(c.trigger_delay_ms, 1000);
        assert_eq!(c.drag_threshold_px, 5);
        assert_eq!(c.suppress.min_text_length, 1);
        assert_eq!(c.suppress.max_text_length, 3)
    }

    #[test]
    fn hot_update_applies_and_normalizes() {
        let shared = SharedConfig::new(SelectionConfig::default());
        let applied = shared.update(SelectionConfig {
            trigger_mode: TriggerMode::Hotkey,
            trigger_delay_ms: 50,
            ..Default::default()
        });
        assert_eq!(shared.get().trigger_mode, TriggerMode::Hotkey);
        assert_eq!(applied.trigger_delay_ms, 100);
    }
}
