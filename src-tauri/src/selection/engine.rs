//! 取词引擎住循环

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::mpsc;

use crate::selection::config::{SelectionConfig, SharedConfig, TriggerMode};
use crate::selection::event::{CaptureError, CaptureSource, SelectionData, SelectionInputEvent};
use crate::selection::platform;
use crate::selection::state::{Effect, SelectionStateMachine};
use crate::selection::suppress::{self, SuppressReason};
#[async_trait::async_trait]
pub trait TextCapturer: Send + Sync {
    async fn capture(&self) -> Result<SelectionData, CaptureError>;
}

#[derive(Debug, Clone, PartialEq)]
pub enum EngineEvent {
    /// 取词成功且通过抑制规则
    Captured(SelectionData),
    /// 取词失败
    CaptureFailed(CaptureError),
    /// 命中抑制规则
    Suppressed(SuppressReason),
}

const CLIPBOARD_RECAPTURE_COOLDOWN: Duration = Duration::from_millis(600);

pub struct SelectionEngine {
    state_machine: SelectionStateMachine,
    config: SharedConfig,
    capturer: Arc<dyn TextCapturer>,
    foreground_app: ForegroundAppFn,
    cooldown_until: Option<tokio::time::Instant>,
}

/// 获取前台应用标识（测试可注入 Mock）
pub type ForegroundAppFn = Arc<dyn Fn() -> Option<String> + Send + Sync>;

impl SelectionEngine {
    pub fn new(config: SharedConfig, capturer: Arc<dyn TextCapturer>) -> Self {
        Self::with_foreground_app(config, capturer, Arc::new(platform::foreground_app_id))
    }

    pub fn with_foreground_app(
        config: SharedConfig,
        capturer: Arc<dyn TextCapturer>,
        foreground_app: ForegroundAppFn,
    ) -> Self {
        Self {
            state_machine: SelectionStateMachine::new(),
            config,
            capturer,
            foreground_app,
            cooldown_until: None,
        }
    }

    /// 引擎主循环
    pub async fn run(
        mut self,
        tx: mpsc::Sender<SelectionInputEvent>,
        mut rx: mpsc::Receiver<SelectionInputEvent>,
        events: mpsc::Sender<EngineEvent>,
    ) {
        while let Some(event) = rx.recv().await {
            let cfg = self.config.get();
            let effect = self.state_machine.handle(event, &cfg);

            match effect {
                Effect::None => {}
                Effect::ArmDebounce {
                    generation,
                    delay_ms,
                } => {
                    let tx = tx.clone();
                    tokio::spawn(async move {
                        tokio::time::sleep(Duration::from_millis(u64::from(delay_ms))).await;
                        let _ = tx
                            .send(SelectionInputEvent::DebounceFired { generation })
                            .await;
                    });
                }
                Effect::TriggerCapture => self.on_trigger(&event, &cfg, &events).await,
            }
        }
    }

    /// 处理取词触发
    async fn on_trigger(
        &mut self,
        event: &SelectionInputEvent,
        cfg: &SelectionConfig,
        events: &mpsc::Sender<EngineEvent>,
    ) {
        if cfg.trigger_mode == TriggerMode::Hotkey
            && matches!(event, SelectionInputEvent::DebounceFired { .. })
        {
            return;
        }

        if let Some(until) = self.cooldown_until {
            if tokio::time::Instant::now() < until {
                return;
            }
            self.cooldown_until = None;
        }

        // 应用名单规则先于取词：黑名单应用完全不接触（尤其不能注入复制快捷键）
        let foreground_app = (self.foreground_app)();
        if let Some(reason) = suppress::app_reason(foreground_app.as_deref(), &cfg.suppress) {
            let _ = events.send(EngineEvent::Suppressed(reason)).await;
            return;
        }

        match self.capturer.capture().await {
            Ok(data) => {
                if data.source == CaptureSource::Clipboard {
                    self.cooldown_until =
                        Some(tokio::time::Instant::now() + CLIPBOARD_RECAPTURE_COOLDOWN);
                }
                match suppress::text_reason(&data.text, &cfg.suppress) {
                    None => {
                        let _ = events.send(EngineEvent::Captured(data)).await;
                    }
                    Some(reason) => {
                        let _ = events.send(EngineEvent::Suppressed(reason)).await;
                    }
                }
            }
            Err(e) => {
                let _ = events.send(EngineEvent::CaptureFailed(e)).await;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;

    use super::*;
    use crate::selection::config::SelectionConfig;
    use crate::selection::event::{CaptureSource, Key};

    struct MockCapturer {
        results: Mutex<VecDeque<Result<SelectionData, CaptureError>>>,
        calls: AtomicUsize,
    }
    impl MockCapturer {
        fn new(results: Vec<Result<SelectionData, CaptureError>>) -> Self {
            Self {
                results: Mutex::new(results.into()),
                calls: AtomicUsize::new(0),
            }
        }
    }

    #[async_trait::async_trait]
    impl TextCapturer for MockCapturer {
        async fn capture(&self) -> Result<SelectionData, CaptureError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.results
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or(Err(CaptureError::Timeout))
        }
    }

    fn ok(text: &str) -> Result<SelectionData, CaptureError> {
        Ok(SelectionData {
            text: text.into(),
            source: CaptureSource::Accessibility,
        })
    }

    fn down(x: i32, y: i32) -> SelectionInputEvent {
        SelectionInputEvent::MouseDown { pos: (x, y) }
    }

    fn up(x: i32, y: i32) -> SelectionInputEvent {
        SelectionInputEvent::MouseUp { pos: (x, y) }
    }

    fn setup(
        config: SelectionConfig,
        results: Vec<Result<SelectionData, CaptureError>>,
    ) -> (
        mpsc::Sender<SelectionInputEvent>,
        mpsc::Receiver<EngineEvent>,
    ) {
        let (tx, rx) = mpsc::channel(32);
        let (etx, erx) = mpsc::channel(32);
        let engine = SelectionEngine::with_foreground_app(
            SharedConfig::new(config),
            Arc::new(MockCapturer::new(results)),
            Arc::new(|| None),
        );
        tokio::spawn(engine.run(tx.clone(), rx, etx));
        (tx, erx)
    }

    async fn recv_event(erx: &mut mpsc::Receiver<EngineEvent>) -> Option<EngineEvent> {
        tokio::time::timeout(Duration::from_secs(1), erx.recv())
            .await
            .ok()
            .flatten()
    }

    #[tokio::test(start_paused = true)]
    async fn blacklisted_app_suppressed_before_capture() {
        let (tx, rx) = mpsc::channel(32);
        let (etx, mut erx) = mpsc::channel(32);
        let capturer = Arc::new(MockCapturer::new(vec![ok("hello")]));
        let engine = SelectionEngine::with_foreground_app(
            SharedConfig::new(SelectionConfig::default()),
            capturer.clone(),
            Arc::new(|| Some("windowsterminal.exe".into())),
        );
        tokio::spawn(engine.run(tx.clone(), rx, etx));

        tx.send(down(0, 0)).await.unwrap();
        tx.send(up(0, 0)).await.unwrap();
        drop(tx);

        assert_eq!(
            recv_event(&mut erx).await,
            Some(EngineEvent::Suppressed(SuppressReason::AppBlacklisted))
        );
        // 黑名单应用不应触发取词（不注入复制快捷键）
        assert_eq!(capturer.calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test(start_paused = true)]
    async fn debounce_flow_captures_text() {
        let (tx, mut erx) = setup(SelectionConfig::default(), vec![ok("hello")]);

        tx.send(down(0, 0)).await.unwrap();
        tx.send(up(0, 0)).await.unwrap();
        drop(tx); // 关闭输入，让引擎在定时器回送后自然结束

        assert_eq!(
            recv_event(&mut erx).await,
            Some(EngineEvent::Captured(SelectionData {
                text: "hello".into(),
                source: CaptureSource::Accessibility,
            }))
        )
    }

    #[tokio::test(start_paused = true)]
    async fn double_click_captures_once() {
        let (tx, mut erx) = setup(SelectionConfig::default(), vec![ok("first"), ok("second")]);

        tx.send(down(0, 0)).await.unwrap();
        tx.send(up(0, 0)).await.unwrap();
        tx.send(down(5, 5)).await.unwrap();
        tx.send(up(0, 0)).await.unwrap();
        drop(tx);

        assert_eq!(
            recv_event(&mut erx).await,
            Some(EngineEvent::Captured(SelectionData {
                text: "first".into(),
                source: CaptureSource::Accessibility,
            }))
        );
        assert_eq!(recv_event(&mut erx).await, None);
    }

    #[tokio::test(start_paused = true)]
    async fn oversized_text_suppressed() {
        let long = "a".repeat(5001);
        let (tx, mut erx) = setup(SelectionConfig::default(), vec![ok(&long)]);

        tx.send(down(0, 0)).await.unwrap();
        tx.send(up(0, 0)).await.unwrap();
        drop(tx);

        assert_eq!(
            recv_event(&mut erx).await,
            Some(EngineEvent::Suppressed(SuppressReason::TextTooLong))
        );
    }

    #[tokio::test(start_paused = true)]
    async fn capture_failure_reported() {
        let (tx, mut erx) = setup(
            SelectionConfig::default(),
            vec![Err(CaptureError::A11yUnavailable)],
        );

        tx.send(down(0, 0)).await.unwrap();
        tx.send(up(0, 0)).await.unwrap();
        drop(tx);

        assert_eq!(
            recv_event(&mut erx).await,
            Some(EngineEvent::CaptureFailed(CaptureError::A11yUnavailable))
        );
    }

    #[tokio::test(start_paused = true)]
    async fn hotkey_mode_skips_debounce_but_allows_ctrl_c() {
        let config = SelectionConfig {
            trigger_mode: TriggerMode::Hotkey,
            ..Default::default()
        };
        let (tx, mut erx) = setup(config, vec![ok("hello")]);

        tx.send(down(0, 0)).await.unwrap();
        tx.send(up(0, 0)).await.unwrap();
        tokio::time::sleep(Duration::from_millis(400)).await;

        tx.send(SelectionInputEvent::KeyDown {
            key: Key::C,
            shift: false,
            ctrl: true,
        })
        .await
        .unwrap();
        drop(tx);

        assert_eq!(
            recv_event(&mut erx).await,
            Some(EngineEvent::Captured(SelectionData {
                text: "hello".into(),
                source: CaptureSource::Accessibility,
            }))
        );
        assert_eq!(recv_event(&mut erx).await, None);
    }

    fn ok_clipboard(text: &str) -> Result<SelectionData, CaptureError> {
        Ok(SelectionData {
            text: text.into(),
            source: CaptureSource::Clipboard,
        })
    }

    #[tokio::test(start_paused = true)]
    async fn clipboard_capture_swallows_injected_echo() {
        let (tx, mut erx) = setup(
            SelectionConfig::default(),
            vec![ok_clipboard("hello"), Err(CaptureError::Timeout)],
        );

        tx.send(down(0, 0)).await.unwrap();
        tx.send(up(0, 0)).await.unwrap();
        tokio::time::sleep(Duration::from_millis(300)).await;
        tx.send(down(0, 0)).await.unwrap();
        tx.send(up(0, 0)).await.unwrap();
        tokio::time::sleep(Duration::from_millis(300)).await;
        drop(tx);

        assert_eq!(
            recv_event(&mut erx).await,
            Some(EngineEvent::Captured(SelectionData {
                text: "hello".into(),
                source: CaptureSource::Clipboard,
            }))
        );
        assert_eq!(recv_event(&mut erx).await, None);
    }
}
