use glance_lib::selection::{config::{SelectionConfig, SharedConfig}, engine::{EngineEvent, SelectionEngine}, event::SelectionInputEvent, input_monitor::spawn_input_monitor, platform};

#[tokio::main(flavor = "current_thread")]
async fn main() -> std::io::Result<()> {
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
            EngineEvent::CaptureFailed(e) => {
                let app = platform::foreground_app_id().unwrap_or_else(|| "未知".into());
                println!("CaptureFailed({e}) foreground={app}");
            }
            other => println!("{other:?}"),
        }
    }
    Ok(())
}