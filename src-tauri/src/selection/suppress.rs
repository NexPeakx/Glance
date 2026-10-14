//! 抑制规则：判断取词结果是否应触发翻译
//! 状态机保证 `TriggerCapture` 时选区已稳定，本模块只做应用规则与文本长度检查

use crate::selection::config::SuppressConfig;

/// 抑制原因
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SuppressReason {
    /// 文本长度低于 `min_text_length`
    TextTooShort,
    /// 文本长度超过 `max_text_length`
    TextTooLong,
    /// 命中应用黑名单
    AppBlacklisted,
    /// 白名单模式下不在白名单中
    NotInWhitelist,
}

/// 抑制评估结果
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// 允许触发翻译
    Allow,
    /// 抑制，携带原因
    Suppress(SuppressReason),
}

/// 应用标识是否命中列表（忽略大小写和首尾空白）
fn app_in_list(app: &str, list: &[String]) -> bool {
    let app = app.trim().to_lowercase();
    list.iter().any(|e| e.trim().to_lowercase() == app)
}

/// 仅评估应用名单规则（取词前调用，避免对黑名单应用注入复制快捷键）
pub fn app_reason(foreground_app: Option<&str>, cfg: &SuppressConfig) -> Option<SuppressReason> {
    if cfg.whitelist_mode {
        let allowed = foreground_app.is_some_and(|app| app_in_list(app, &cfg.app_whitelist));
        if !allowed {
            return Some(SuppressReason::NotInWhitelist);
        }
    } else if let Some(app) = foreground_app {
        if app_in_list(app, &cfg.app_blacklist) {
            return Some(SuppressReason::AppBlacklisted);
        }
    }
    None
}

/// 仅评估文本规则（取词后调用）
pub fn text_reason(text: &str, cfg: &SuppressConfig) -> Option<SuppressReason> {
    let len = text.chars().count();
    if len > cfg.max_text_length {
        return Some(SuppressReason::TextTooLong);
    }
    if len < cfg.min_text_length {
        return Some(SuppressReason::TextTooShort);
    }
    None
}

pub fn evaluate(text: &str, foreground_app: Option<&str>, cfg: &SuppressConfig) -> Decision {
    if let Some(reason) = app_reason(foreground_app, cfg) {
        return Decision::Suppress(reason);
    }
    if let Some(reason) = text_reason(text, cfg) {
        return Decision::Suppress(reason);
    }
    Decision::Allow
}

#[cfg(test)]

mod tests {
    use super::*;

    fn cfg() -> SuppressConfig {
        SuppressConfig::default()
    }

    fn cfg_with_whitelist(mode: bool, list: &[&str]) -> SuppressConfig {
        SuppressConfig {
            whitelist_mode: mode,
            app_whitelist: list.iter().map(|s| s.to_string()).collect(),
            ..SuppressConfig::default()
        }
    }

    // 文本长度

    #[test]
    fn empty_text_suppressed() {
        assert_eq!(
            evaluate("", None, &cfg()),
            Decision::Suppress(SuppressReason::TextTooShort)
        )
    }

    #[test]
    fn length_boundaries() {
        let c = SuppressConfig {
            min_text_length: 2,
            max_text_length: 5,
            ..SuppressConfig::default()
        };
        assert_eq!(
            evaluate("a", None, &c),
            Decision::Suppress(SuppressReason::TextTooShort)
        );
        assert_eq!(evaluate("ab", None, &c), Decision::Allow);
        assert_eq!(evaluate("abcd", None, &c), Decision::Allow);
        assert_eq!(
            evaluate("abcdef", None, &c),
            Decision::Suppress(SuppressReason::TextTooLong)
        );
    }

    #[test]
    fn cjk_counted_by_chars_not_bytes() {
        // 3 个汉字，UTF-8 下 len() 是 9
        assert_eq!(evaluate("你好啊", None, &cfg()), Decision::Allow);
    }

    // 应用黑名单

    #[test]
    fn blacklisted_app_suppressed_case_insensitive() {
        assert_eq!(
            evaluate("hello", Some("KeepAss.EXE"), &cfg()),
            Decision::Suppress(SuppressReason::AppBlacklisted)
        )
    }

    #[test]
    fn app_none_fails_open_in_blacklist_mode() {
        assert_eq!(evaluate("hello", None, &cfg()), Decision::Allow)
    }

    // 应用白名单

    #[test]
    fn whitelist_mode_allows_listed_only() {
        let c = cfg_with_whitelist(true, &["safari"]);
        assert_eq!(evaluate("hello", Some("safari"), &c), Decision::Allow);
        assert_eq!(
            evaluate("hello", Some("terminal"), &c),
            Decision::Suppress(SuppressReason::NotInWhitelist)
        );
        assert_eq!(
            evaluate("hello", None, &c),
            Decision::Suppress(SuppressReason::NotInWhitelist)
        )
    }

    #[test]
    fn whitelist_inactive_when_mode_off() {
        let c = cfg_with_whitelist(false, &["safari"]);
        assert_eq!(evaluate("hello", Some("terminal"), &c), Decision::Allow)
    }

    #[test]
    fn whitelist_pass_then_length_still_applies() {
        let c = cfg_with_whitelist(true, &["safari"]);
        assert_eq!(
            evaluate("", Some("safari"), &c),
            Decision::Suppress(SuppressReason::TextTooShort)
        )
    }
}
