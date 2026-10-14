use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Shift,
    ArrowLeft,
    ArrowRight,
    ArrowUp,
    ArrowDown,
    C,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionInputEvent {
    MouseDown { pos: (i32, i32) },
    MouseMove { pos: (i32, i32) },
    MouseUp { pos: (i32, i32) },
    KeyDown { key: Key, shift: bool, ctrl: bool },
    KeyUp { key: Key, shift: bool, ctrl: bool },
    DebounceFired { generation: u64 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum CaptureSource {
    Accessibility,
    Clipboard,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SelectionData {
    pub text: String,
    pub source: CaptureSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptureError {
    PermissionDenied,    // 辅助功能权限未授予
    A11yUnavailable,     // UIA 拿不到
    SensitiveField,      // 密码框等敏感字段：禁止取词与剪贴板回退
    ClipboardDisabled,   // 配置禁用了回退
    ClipboardOpen,       // 无法打开/清空剪贴板（常被其他进程占用）
    CopyInjectionFailed, // 模拟 Ctrl+C 发送失败
    ClipboardEmpty,      // 超时内剪贴板未出现新内容（目标不支持复制或太慢）
    Timeout,
}
impl std::fmt::Display for CaptureError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CaptureError::A11yUnavailable => write!(f, "accessibility API unavailable"),
            CaptureError::ClipboardDisabled => write!(f, "clipboard fallback disabled"),
            CaptureError::ClipboardOpen => {
                write!(f, "clipboard open failed (locked by another process?)")
            }
            CaptureError::CopyInjectionFailed => write!(f, "failed to inject copy shortcut"),
            CaptureError::ClipboardEmpty => write!(f, "no new clipboard content within timeout"),
            CaptureError::Timeout => write!(f, "capture timed out"),
            CaptureError::PermissionDenied => write!(f, "accessibility permission not granted"),
            CaptureError::SensitiveField => write!(f, "sensitive field (e.g. password box)"),
        }
    }
}
