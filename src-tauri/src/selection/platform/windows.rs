//! Windows 取词实现：UIA 直读 -> 50ms 重试 -> 剪切板回退

use std::{thread, time::Duration};

use tokio::time::Instant;
use windows::{
    core::PWSTR,
    Win32::{
        Foundation::{CloseHandle, HWND},
        System::{
            Com::{
                CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER,
                COINIT_MULTITHREADED,
            },
            Threading::{
                OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
                PROCESS_QUERY_LIMITED_INFORMATION,
            },
        },
        UI::{
            Accessibility::{
                CUIAutomation, IUIAutomation, IUIAutomationElement, IUIAutomationTextPattern,
                IUIAutomationValuePattern, UIA_TextPatternId, UIA_ValuePatternId,
            },
            Input::KeyboardAndMouse::{
                SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS,
                KEYEVENTF_KEYUP, VIRTUAL_KEY, VK_CONTROL,
            },
            WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId},
        },
    },
};

use crate::selection::{
    config::SharedConfig,
    engine::TextCapturer,
    event::{CaptureError, CaptureSource, SelectionData},
};

/// UIA 直读失败后的重试等待
const UIA_RETRY_DELAY: Duration = Duration::from_millis(50);
const CLIPBOARD_WAIT: Duration = Duration::from_millis(400);
const CLIPBOARD_POLL: Duration = Duration::from_millis(25);

const VK_C: VIRTUAL_KEY = VIRTUAL_KEY(0x43);

#[derive(Clone)]
pub struct WindowsCapturer {
    config: SharedConfig,
}

impl WindowsCapturer {
    pub fn new(config: SharedConfig) -> Self {
        Self { config }
    }

    fn capture_blocking(&self) -> Result<SelectionData, CaptureError> {
        for attempt in 0..2 {
            if attempt > 0 {
                thread::sleep(UIA_RETRY_DELAY);
            }
            match with_com(uia_selected_text) {
                Ok(Some(text)) => {
                    return Ok(SelectionData {
                        text,
                        source: CaptureSource::Accessibility,
                    });
                }
                Err(CaptureError::SensitiveField) => return Err(CaptureError::SensitiveField),
                _ => {}
            }
        }

        if !self.config.get().clipboard_fallback {
            return Err(CaptureError::ClipboardDisabled);
        }
        clipboard_capture().map(|text| SelectionData {
            text,
            source: CaptureSource::Clipboard,
        })
    }
}

#[async_trait::async_trait]
impl TextCapturer for WindowsCapturer {
    async fn capture(&self) -> Result<SelectionData, CaptureError> {
        let this = self.clone();
        match tokio::task::spawn_blocking(move || this.capture_blocking()).await {
            Ok(result) => result,
            Err(_) => Err(CaptureError::Timeout),
        }
    }
}

fn with_com<T>(f: impl FnOnce() -> Result<T, CaptureError>) -> Result<T, CaptureError> {
    let hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    if hr.is_err() {
        return Err(CaptureError::A11yUnavailable);
    }
    let out = f();
    unsafe {
        CoUninitialize();
    };
    out
}

fn uia_selected_text() -> Result<Option<String>, CaptureError> {
    unsafe {
        let automation: IUIAutomation =
            CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER)
                .map_err(|_| CaptureError::A11yUnavailable)?;
        let element: IUIAutomationElement = automation
            .GetFocusedElement()
            .map_err(|_| CaptureError::A11yUnavailable)?;

        if element
            .CurrentIsPassword()
            .map_err(|_| CaptureError::A11yUnavailable)?
            .as_bool()
        {
            return Err(CaptureError::SensitiveField);
        }
        Ok(text_from_element(&element))
    }
}

unsafe fn text_from_element(element: &IUIAutomationElement) -> Option<String> {
    unsafe {
        if let Ok(pattern) =
            element.GetCurrentPatternAs::<IUIAutomationTextPattern>(UIA_TextPatternId)
        {
            if let Ok(ranges) = pattern.GetSelection() {
                if let Ok(count) = ranges.Length() {
                    let mut texts = Vec::new();
                    for i in 0..count {
                        let Ok(range) = ranges.GetElement(i) else {
                            continue;
                        };
                        let Ok(text) = range.GetText(-1) else {
                            continue;
                        };
                        if !text.is_empty() {
                            texts.push(text.to_string());
                        }
                    }
                    if !texts.is_empty() {
                        return Some(texts.join("\n"));
                    }
                }
            }
        }

        if let Ok(pattern) =
            element.GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId)
        {
            if let Ok(value) = pattern.CurrentValue() {
                let value = value.to_string();
                if !value.trim().is_empty() {
                    return Some(value);
                }
            }
        }
        None
    }
}

fn clipboard_capture() -> Result<String, CaptureError> {
    let mut cb = arboard::Clipboard::new().map_err(|_| CaptureError::ClipboardOpen)?;

    let original = cb.get_text().ok();
    cb.clear().map_err(|_| CaptureError::ClipboardOpen)?;

    if !post_ctrl_c() {
        if let Some(original) = original {
            let _ = cb.set_text(original);
        }
        return Err(CaptureError::CopyInjectionFailed);
    }

    let deadline = Instant::now() + CLIPBOARD_WAIT;
    let mut captured = None;
    while Instant::now() < deadline {
        thread::sleep(CLIPBOARD_POLL);
        if let Ok(text) = cb.get_text() {
            if !text.trim().is_empty() {
                captured = Some(text);
                break;
            }
        }
    }

    if let Some(original) = original {
        let _ = cb.set_text(original);
    }

    captured.ok_or(CaptureError::ClipboardEmpty)
}

fn post_ctrl_c() -> bool {
    let key = |vk: VIRTUAL_KEY, up: bool| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: 0,
                dwFlags: if up {
                    KEYEVENTF_KEYUP
                } else {
                    KEYBD_EVENT_FLAGS(0)
                },
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    let inputs = [
        key(VK_CONTROL, false),
        key(VK_C, false),
        key(VK_C, true),
        key(VK_CONTROL, true),
    ];
    unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) == inputs.len() as u32 }
}

pub fn foreground_app_id() -> Option<String> {
    unsafe {
        let hwnd: HWND = GetForegroundWindow();
        if hwnd.is_invalid() {
            return None;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == 0 {
            return None;
        }

        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 512];
        let mut len = buf.len() as u32;
        let path = QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            PWSTR(buf.as_mut_ptr()),
            &mut len,
        )
        .ok()
        .map(|_| String::from_utf16_lossy(&buf[..len as usize]));
        let _ = CloseHandle(process);

        image_name_to_id(&path?)
    }
}

fn image_name_to_id(path: &str) -> Option<String> {
    let name = path.rsplit(['\\', '/']).next()?.trim().to_lowercase();
    if name.is_empty() {
        None
    } else {
        Some(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_name_extraction() {
        assert_eq!(
            image_name_to_id("C:\\Windows\\explorer.exe"),
            Some("explorer.exe".into())
        );
        assert_eq!(image_name_to_id("usr/bin/app"), Some("app".into()));
        assert_eq!(image_name_to_id("app.exe"), Some("app.exe".into()));
        assert_eq!(image_name_to_id("   "), None);
        assert_eq!(image_name_to_id(""), None);
    }

    #[test]
    fn foreground_app_is_lowercase_process_name() {
        let id = foreground_app_id().expect("应能取到");
        println!("foreground_app: {id}");
        assert!(!id.is_empty());
        assert_eq!(id, id.to_lowercase());
    }

    #[test]
    fn uia_chain_smoke() {
        let _ = with_com(uia_selected_text).expect("COM 初始化不应该失败");
    }
}
