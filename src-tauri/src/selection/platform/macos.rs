//! macos 取词实现：AXAPI 直读 -> 50ms 重试 -> 剪贴板回退

use std::ffi::c_void;
use std::ptr;
use std::thread;
use std::time::{Duration, Instant};

use core_foundation::base::{CFType, TCFType};
use core_foundation::boolean::CFBoolean;
use core_foundation::dictionary::CFDictionary;
use core_foundation::string::{CFString, CFStringRef};
use objc2::rc::autoreleasepool;
use objc2_app_kit::NSWorkspace;

use crate::selection::config::SharedConfig;
use crate::selection::engine::TextCapturer;
use crate::selection::event::{CaptureError, CaptureSource, SelectionData};

/// AXAPI 直读失败后的重试等待
const AX_RETRY_DELAY: Duration = Duration::from_millis(50);
/// AXAPI 调用消息超时
const AX_MESSAGING_TIMEOUT: f32 = 1.0;
const CLIPBOARD_WAIT: Duration = Duration::from_millis(400);
const CLIPBOARD_POLL: Duration = Duration::from_millis(25);

/// AXAPI FFI
type AXError = i32;

const K_AX_VALUE_RANGE_TYPE: u32 = 4;

#[repr(C)]
#[derive(Clone, Copy)]
struct CFRange {
    location: isize,
    length: isize,
}

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXIsProcessTrustedWithOptions(options: *const c_void) -> u8;
    fn AXUIElementCreateSystemWide() -> *mut c_void;
    fn AXUIElementSetMessagingTimeout(element: *mut c_void, timeout: f32);
    fn AXUIElementCopyAttributeValue(
        element: *mut c_void,
        attribute: *const c_void,
        value: *mut *mut c_void,
    ) -> AXError;
    fn AXUIElementCopyParameterizedAttributeValue(
        element: *mut c_void,
        attribute: *const c_void,
        parameter: *const c_void,
        result: *mut *mut c_void,
    ) -> AXError;
    fn AXValueCreate(value_type: u32, value: *const CFRange) -> *mut c_void;
    fn AXValueGetValue(value: *mut c_void, value_type: u32, buffer: *mut CFRange) -> u8;
}

const K_CG_EVENT_FLAG_MASK_COMMAND: u64 = 1 << 20;
const K_CG_HID_EVENT_TAP: u32 = 0;
const K_VK_ANSI_C: u32 = 8;

extern "C" {
    fn CGEventCreateKeyboardEvent(
        source: *mut c_void,
        key_code: u32,
        key_down: bool,
    ) -> *mut c_void;
    fn CGEventSetFlags(event: *mut c_void, flags: u64);
    fn CGEventPost(tap: u32, event: *mut c_void);
    fn CFRelease(cf: *mut c_void);
}

/// 权限检查
pub fn is_accessibility_trusted(prompt: bool) -> bool {
    let key = CFString::new("AXTrustedCheckOptionPrompt");
    let value = if prompt {
        CFBoolean::true_value()
    } else {
        CFBoolean::false_value()
    };
    let options = CFDictionary::from_CFType_pairs(&[(key, value)]);
    unsafe { AXIsProcessTrustedWithOptions(options.as_concrete_TypeRef() as *const c_void) != 0 }
}

unsafe fn copy_string_attribute(element: *mut c_void, name: &str) -> Result<String, ()> {
    let mut value: *mut c_void = ptr::null_mut();
    let err = AXUIElementCopyAttributeValue(
        element,
        CFString::new(name).as_concrete_TypeRef() as *const c_void,
        &mut value,
    );
    if err != 0 || value.is_null() {
        return Err(());
    }
    Ok(CFString::wrap_under_create_rule(value as CFStringRef).to_string())
}

unsafe fn copy_attribute(element: *mut c_void, name: &str) -> Result<CFType, ()> {
    let mut value: *mut c_void = ptr::null_mut();
    let err = AXUIElementCopyAttributeValue(
        element,
        CFString::new(name).as_concrete_TypeRef() as *const c_void,
        &mut value,
    );
    if err != 0 || value.is_null() {
        return Err(());
    }
    Ok(CFType::wrap_under_create_rule(value as *const c_void))
}

unsafe fn ax_selected_text() -> Result<String, ()> {
    let system_wide = AXUIElementCreateSystemWide();
    if system_wide.is_null() {
        return Err(());
    }
    let system_wide = CFType::wrap_under_create_rule(system_wide as *const c_void);

    AXUIElementSetMessagingTimeout(
        system_wide.as_CFTypeRef() as *mut c_void,
        AX_MESSAGING_TIMEOUT,
    );

    let system_wide = system_wide.as_CFTypeRef() as *mut c_void;

    // 焦点元素
    let focused = copy_attribute(system_wide, "AXFocusedUIElement")?;
    AXUIElementSetMessagingTimeout(focused.as_CFTypeRef() as *mut c_void, AX_MESSAGING_TIMEOUT);
    let focused = focused.as_CFTypeRef() as *mut c_void;

    if let Ok(text) = copy_string_attribute(focused, "AXSelectedText") {
        if !text.is_empty() {
            return Ok(text);
        }
    }

    let range_value = copy_attribute(focused, "AXSelectedTextRange")?;
    let mut range = CFRange {
        location: 0,
        length: 0,
    };
    if AXValueGetValue(
        range_value.as_CFTypeRef() as *mut c_void,
        K_AX_VALUE_RANGE_TYPE,
        &mut range,
    ) == 0
        || range.length <= 0
    {
        return Err(());
    }

    let range_param = AXValueCreate(K_AX_VALUE_RANGE_TYPE, &range);
    if range_param.is_null() {
        return Err(());
    }
    let range_param = CFType::wrap_under_create_rule(range_param as *const c_void);

    let mut out: *mut c_void = ptr::null_mut();
    let err = AXUIElementCopyParameterizedAttributeValue(
        focused,
        CFString::new("AXStringForRange").as_concrete_TypeRef() as *const c_void,
        range_param.as_CFTypeRef(),
        &mut out,
    );
    if err != 0 || out.is_null() {
        return Err(());
    }
    let text = CFString::wrap_under_create_rule(out as CFStringRef).to_string();
    if text.is_empty() {
        Err(())
    } else {
        Ok(text)
    }
}

fn post_cmd_c() {
    unsafe {
        for key_down in [true, false] {
            let event = CGEventCreateKeyboardEvent(ptr::null_mut(), K_VK_ANSI_C, key_down);
            if event.is_null() {
                continue;
            }
            CGEventSetFlags(event, K_CG_EVENT_FLAG_MASK_COMMAND);
            CGEventPost(K_CG_HID_EVENT_TAP, event);
            CFRelease(event);
        }
    }
}

fn clipboard_capture() -> Result<String, CaptureError> {
    let mut cb = arboard::Clipboard::new().map_err(|_| CaptureError::ClipboardFailed)?;

    let original = cb.get_text().ok();
    cb.clear().map_err(|_| CaptureError::ClipboardFailed)?;

    post_cmd_c();

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

    captured.ok_or(CaptureError::ClipboardFailed)
}

/// macOS 取词器
#[derive(Clone)]
pub struct MacCapturer {
    config: SharedConfig,
}

impl MacCapturer {
    pub fn new(config: SharedConfig) -> Self {
        Self { config }
    }

    fn capture_blocking(&self) -> Result<SelectionData, CaptureError> {
        if !is_accessibility_trusted(false) {
            return Err(CaptureError::PermissionDenied);
        }

        for attempt in 0..2 {
            if attempt > 0 {
                thread::sleep(AX_RETRY_DELAY);
            }
            if let Ok(text) = unsafe { ax_selected_text() } {
                return Ok(SelectionData {
                    text,
                    source: CaptureSource::Accessibility,
                });
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
impl TextCapturer for MacCapturer {
    async fn capture(&self) -> Result<SelectionData, CaptureError> {
        let this = self.clone();
        match tokio::task::spawn_blocking(move || this.capture_blocking()).await {
            Ok(result) => result,
            Err(_) => Err(CaptureError::Timeout),
        }
    }
}

pub fn foreground_app_id() -> Option<String> {
    autoreleasepool(|_| {
        let app = NSWorkspace::sharedWorkspace().frontmostApplication()?;
        if let Some(bundle_id) = app.bundleIdentifier() {
            return Some(bundle_id.to_string());
        }
        app.localizedName().map(|name| name.to_string())
    })
}
