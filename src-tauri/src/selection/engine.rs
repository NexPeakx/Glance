//! 取词引擎住循环

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::mpsc;

use crate::selection::config::{SelectionConfig, SharedConfig, TriggerMode};
use crate::selection::event::{CaptureError, SelectionData, SelectionInputEvent};
use crate::selection::state::{Effect, SelectionStateMachine};
use crate::selection::suppress::{self, Decision, SuppressReason};

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

pub struct SelectionEngine {
    state_machine: SelectionStateMachine,
    config: SharedConfig,
    capturer: Arc<dyn TextCapturer>,
}

impl SelectionEngine {
    pub fn new(config: SharedConfig, capturer: Arc<dyn TextCapturer>) -> Self {
        Self {
            state_machine: SelectionStateMachine::new(),
            config,
            capturer,
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

        match self.capturer.capture().await {
            Ok(data) => {
                // TODO(平台层)
                match suppress::evaluate(&data.text, None, &cfg.suppress) {
                    Decision::Allow => {
                        let _ = events.send(EngineEvent::Captured(data)).await;
                    }
                    Decision::Suppress(reason) => {
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
    use std::sync::Mutex;

    use super::*;
    use crate::selection::config::SelectionConfig;
    use crate::selection::event::{CaptureSource, Key};

    struct MockCapturer {
        results: Mutex<VecDeque<Result<SelectionData, CaptureError>>>,
    }
    impl MockCapturer {
        fn new(results: Vec<Result<SelectionData, CaptureError>>) -> Self {
            Self {
                results: Mutex::new(results.into()),
            }
        }
    }

    #[async_trait::async_trait]
    impl TextCapturer for MockCapturer {
        async fn capture(&self) -> Result<SelectionData, CaptureError> {
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
        let engine = SelectionEngine::new(
            SharedConfig::new(config),
            Arc::new(MockCapturer::new(results)),
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
}
