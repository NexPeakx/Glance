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

#[derive(Debug, Clone, PartialEq)]
pub enum CaptureError {
    PermissionDenied,  // 辅助功能权限未授予
    A11yUnavailable,   // UIA 拿不到
    ClipboardDisabled, // 配置禁用了回退
    ClipboardFailed,
    Timeout,
}
impl std::fmt::Display for CaptureError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CaptureError::A11yUnavailable => write!(f, "accessibility API unavailable"),
            CaptureError::ClipboardDisabled => write!(f, "clipboard fallback disabled"),
            CaptureError::ClipboardFailed => write!(f, "clipboard capture failed"),
            CaptureError::Timeout => write!(f, "capture timed out"),
            CaptureError::PermissionDenied => write!(f, "accessibility permission not granted"),
        }
    }
}
