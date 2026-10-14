//! SelectionEngine <=> Tauri 桥接：应用启动时拉起取词引擎
//! 取词事件转发给前端窗口，并暴露配置与权限相关的 IPC 命令

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::selection::{
    config::{SelectionConfig, SharedConfig},
    engine::{EngineEvent, SelectionEngine},
    event::{CaptureError, CaptureSource, SelectionInputEvent},
    input_monitor::spawn_input_monitor,
    platform,
    suppress::SuppressReason,
};

pub const EVENT_SELECTION: &str = "selection:event";

/// 转发给前端的取词事件
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SelectionEventPayload {
    // 取词成功
    Captured { text: String, source: CaptureSource },
    // 取词失败
    CaptureFailed { error: CaptureError },
    // 命中抑制规则
    Suppressed { reason: SuppressReason },
    // 缺少权限
    NoPermission,
}

/// 划词服务状态，供 IPC 命令访问
pub struct SelectionState {
    pub config: SharedConfig,
}

pub fn init(app: AppHandle) {
    let config = SharedConfig::new(SelectionConfig::default());
    app.manage(SelectionState {
        config: config.clone(),
    });

    // mac 权限预检
    if !platform::is_accessibility_trusted(false) {
        eprintln!("[selection] 未授予辅助功能权限，取词不可用；授权后请重启应用");
        let _ = app.emit(EVENT_SELECTION, SelectionEventPayload::NoPermission);
        return;
    }

    // 1. rdev 全局钩子线程 -> unbounded channel
    let (hook_tx, mut hook_rx) = tokio::sync::mpsc::unbounded_channel::<SelectionInputEvent>();
    if let Err(err) = spawn_input_monitor(hook_tx) {
        eprintln!("[selection] 输入监听启动失败：{err}");
        return;
    }

    // 2. 引擎主循环
    let capturer = platform::create_capturer(config.clone());
    let (engine_in_tx, engine_in_rx) = tokio::sync::mpsc::channel::<SelectionInputEvent>(128);
    let (engine_out_tx, mut engine_out_rx) = tokio::sync::mpsc::channel::<EngineEvent>(32);
    let engine = SelectionEngine::new(config, capturer);

    tauri::async_runtime::spawn(engine.run(engine_in_tx.clone(), engine_in_rx, engine_out_tx));

    // 3. 桥接
    tauri::async_runtime::spawn(async move {
        while let Some(event) = hook_rx.recv().await {
            if engine_in_tx.send(event).await.is_err() {
                break;
            }
        }
    });

    // 4. 引擎事件 -> 前端
    tauri::async_runtime::spawn(async move {
        while let Some(event) = engine_out_rx.recv().await {
            let payload = match event {
                EngineEvent::Captured(data) => SelectionEventPayload::Captured {
                    text: data.text,
                    source: data.source,
                },
                EngineEvent::CaptureFailed(err) => {
                    SelectionEventPayload::CaptureFailed { error: err }
                }
                EngineEvent::Suppressed(reason) => SelectionEventPayload::Suppressed { reason },
            };
            eprintln!("[selection] -> 前端：{payload:?}");
            if let Err(err) = app.emit(EVENT_SELECTION, payload) {
                eprintln!("[selection] 事件转发失败：{err}");
            }
        }
    });
}

/// 检查辅助功能权限
#[tauri::command]
pub fn selection_check_permission(prompt: bool) -> bool {
    platform::is_accessibility_trusted(prompt)
}

/// 读取划词配置
#[tauri::command]
pub fn selection_get_config(state: tauri::State<'_, SelectionState>) -> SelectionConfig {
    state.config.get()
}

/// 热更新划词配置
#[tauri::command]
pub fn selection_update_config(
    state: tauri::State<'_, SelectionState>,
    config: SelectionConfig,
) -> SelectionConfig {
    state.config.update(config)
}
