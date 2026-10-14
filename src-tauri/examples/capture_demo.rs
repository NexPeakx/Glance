use glance_lib::selection::{
    config::{SelectionConfig, SharedConfig},
    engine::{EngineEvent, SelectionEngine},
    event::{CaptureSource, SelectionInputEvent},
    input_monitor::spawn_input_monitor,
    platform,
};

#[tokio::main(flavor = "current_thread")]
async fn main() -> std::io::Result<()> {
    // macOS：未授权「辅助功能」时 rdev 收不到事件、AXAPI 直接失败，先检查并弹授权引导
    if !platform::is_accessibility_trusted(true) {
        eprintln!("未授予「辅助功能」权限：系统设置 → 隐私与安全性 → 辅助功能，");
        eprintln!("勾选运行本程序的终端 App（Terminal/iTerm2/VS Code），重启终端后重试。");
        std::process::exit(1);
    }
    let config = SharedConfig::new(SelectionConfig::default());
    let capturer = platform::create_capturer(config.clone());

    let (raw_tx, mut raw_rx) = tokio::sync::mpsc::unbounded_channel::<SelectionInputEvent>();
    spawn_input_monitor(raw_tx)?;

    let (engine_tx, engine_rx) = tokio::sync::mpsc::channel::<SelectionInputEvent>(32);
    let bridge_tx = engine_tx.clone();
    tokio::spawn(async move {
        while let Some(event) = raw_rx.recv().await {
            if bridge_tx.send(event).await.is_err() {
                break;
            }
        }
    });

    let (event_tx, mut event_rx) = tokio::sync::mpsc::channel(32);
    let engine = SelectionEngine::new(config, capturer);
    tokio::spawn(engine.run(engine_tx.clone(), engine_rx, event_tx));

    println!("监听中：去任意应用中选中文本试试...");
    while let Some(event) = event_rx.recv().await {
        match event {
            EngineEvent::Captured(data) => {
                let source = match data.source {
                    CaptureSource::Accessibility => "AXAPI",
                    CaptureSource::Clipboard => "剪贴板回退",
                };
                println!("Captured[{source}] {:?}", data.text);
            }
            EngineEvent::CaptureFailed(e) => {
                let app = platform::foreground_app_id().unwrap_or_else(|| "未知".into());
                println!("CaptureFailed({e}) foreground={app}");
            }
            other => println!("{other:?}"),
        }
    }
    Ok(())
}
