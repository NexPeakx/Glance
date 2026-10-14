//! 全局输入监听：rdev 钩子线程 -> mpsc -> SelectionEngine

use std::io;
use std::thread;
use std::time::{Duration, Instant};

use rdev::{Button, EventType, Key as RdevKey};
use tokio::sync::mpsc::UnboundedSender;

use crate::selection::event::{Key as SelectionKey, SelectionInputEvent};

/// MouseMove 转发的最小间隔（约 125 Hz）
const MOVE_MIN_INTERVAL: Duration = Duration::from_millis(8);

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct ModifierState {
    shift: bool,
    ctrl: bool,
}

#[derive(Debug, Default)]
struct HookState {
    mods: ModifierState,
    last_cursor: (i32, i32),
    last_move_sent: Option<Instant>,
}

pub fn spawn_input_monitor(tx: UnboundedSender<SelectionInputEvent>) -> io::Result<()> {
    thread::Builder::new()
        .name("input-monitor".into())
        .spawn(move || run_hook(tx))
        .map(|_| ())
}

fn run_hook(tx: UnboundedSender<SelectionInputEvent>) {
    let mut state = HookState::default();

    let result = rdev::listen(move |event: rdev::Event| {
        if let Some(out) = map_event(&event.event_type, &mut state) {
            let _ = tx.send(out);
        }
    });

    if let Err(error) = result {
        eprintln!("[input-monitor] listen 退出： {error:?}")
    }
}

fn map_event(event_type: &EventType, st: &mut HookState) -> Option<SelectionInputEvent> {
    match event_type {
        EventType::MouseMove { x, y } => {
            st.last_cursor = (*x as i32, *y as i32);
            let now = Instant::now();
            if st
                .last_move_sent
                .is_some_and(|t| now.duration_since(t) < MOVE_MIN_INTERVAL)
            {
                return None;
            }
            st.last_move_sent = Some(now);
            Some(SelectionInputEvent::MouseMove {
                pos: st.last_cursor,
            })
        }
        EventType::ButtonPress(Button::Left) => Some(SelectionInputEvent::MouseDown {
            pos: st.last_cursor,
        }),
        EventType::ButtonRelease(Button::Left) => Some(SelectionInputEvent::MouseUp {
            pos: st.last_cursor,
        }),
        EventType::KeyPress(key) => map_key(*key, true, &mut st.mods),
        EventType::KeyRelease(key) => map_key(*key, false, &mut st.mods),
        _ => None,
    }
}

fn map_key(key: RdevKey, is_down: bool, mods: &mut ModifierState) -> Option<SelectionInputEvent> {
    match key {
        RdevKey::ShiftLeft | RdevKey::ShiftRight => mods.shift = is_down,
        RdevKey::ControlLeft | RdevKey::ControlRight => mods.ctrl = is_down,
        _ => {}
    }

    let key = match key {
        RdevKey::ShiftLeft | RdevKey::ShiftRight => SelectionKey::Shift,
        RdevKey::LeftArrow => SelectionKey::ArrowLeft,
        RdevKey::RightArrow => SelectionKey::ArrowRight,
        RdevKey::UpArrow => SelectionKey::ArrowUp,
        RdevKey::DownArrow => SelectionKey::ArrowDown,
        RdevKey::KeyC => SelectionKey::C,
        _ => SelectionKey::Other,
    };

    Some(if is_down {
        SelectionInputEvent::KeyDown {
            key,
            shift: mods.shift,
            ctrl: mods.ctrl,
        }
    } else {
        SelectionInputEvent::KeyUp {
            key,
            shift: mods.shift,
            ctrl: mods.ctrl,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed(st: &mut HookState, event_type: EventType) -> Option<SelectionInputEvent> {
        map_event(&event_type, st)
    }

    fn kd(key: SelectionKey, shift: bool, ctrl: bool) -> SelectionInputEvent {
        SelectionInputEvent::KeyDown { key, shift, ctrl }
    }

    fn ku(key: SelectionKey, shift: bool, ctrl: bool) -> SelectionInputEvent {
        SelectionInputEvent::KeyUp { key, shift, ctrl }
    }

    #[test]
    fn left_click_uses_cached_cursor() {
        let mut st = HookState::default();

        assert_eq!(
            feed(&mut st, EventType::MouseMove { x: 120.0, y: 64.0 }),
            Some(SelectionInputEvent::MouseMove { pos: (120, 64) })
        );
        assert_eq!(
            feed(&mut st, EventType::ButtonPress(Button::Left)),
            Some(SelectionInputEvent::MouseDown { pos: (120, 64) })
        )
    }

    #[test]
    fn moves_throttled_but_cursor_cached() {
        let mut st = HookState::default();
        st.last_move_sent = Some(Instant::now());

        assert_eq!(feed(&mut st, EventType::MouseMove { x: 7.0, y: 7.0 }), None);
        assert_eq!(st.last_cursor, (7, 7));
        assert_eq!(
            feed(&mut st, EventType::ButtonRelease(Button::Left)),
            Some(SelectionInputEvent::MouseUp { pos: (7, 7) })
        )
    }

    #[test]
    fn ctrl_c_sequence_carries_modifier_snapshot() {
        let mut st = HookState::default();

        assert_eq!(
            feed(&mut st, EventType::KeyPress(RdevKey::ControlLeft)),
            Some(kd(SelectionKey::Other, false, true))
        );
        assert_eq!(
            feed(&mut st, EventType::KeyPress(RdevKey::KeyC)),
            Some(kd(SelectionKey::C, false, true))
        );
        assert_eq!(
            feed(&mut st, EventType::KeyRelease(RdevKey::KeyC)),
            Some(ku(SelectionKey::C, false, true))
        );
    }

    #[test]
    fn keyboard_selection_sequence() {
        let mut st = HookState::default();

        assert_eq!(
            feed(&mut st, EventType::KeyPress(RdevKey::ShiftLeft)),
            Some(kd(SelectionKey::Shift, true, false))
        );
        assert_eq!(
            feed(&mut st, EventType::KeyPress(RdevKey::RightArrow)),
            Some(kd(SelectionKey::ArrowRight, true, false))
        );
        assert_eq!(
            feed(&mut st, EventType::KeyRelease(RdevKey::ShiftLeft)),
            Some(ku(SelectionKey::Shift, false, false))
        );
    }

    #[test]
    fn plain_typing_maps_to_other() {
        let mut st = HookState::default();

        assert_eq!(
            feed(&mut st, EventType::KeyPress(RdevKey::KeyA)),
            Some(kd(SelectionKey::Other, false, false))
        );
        assert_eq!(
            feed(&mut st, EventType::KeyRelease(RdevKey::KeyA)),
            Some(ku(SelectionKey::Other, false, false))
        );
    }

    #[test]
    fn alt_gr_does_not_look_like_ctrl() {
        let mut st = HookState::default();

        feed(&mut st, EventType::KeyPress(RdevKey::AltGr));
        assert_eq!(
            feed(&mut st, EventType::KeyPress(RdevKey::KeyE)),
            Some(kd(SelectionKey::Other, false, false))
        );
    }

    #[test]
    fn wheel_and_other_buttons_ignored() {
        let mut st = HookState::default();

        assert_eq!(feed(&mut st, EventType::ButtonPress(Button::Right)), None);
        assert_eq!(
            feed(&mut st, EventType::ButtonPress(Button::Unknown(9))),
            None
        );
        assert_eq!(
            feed(
                &mut st,
                EventType::Wheel {
                    delta_x: 0,
                    delta_y: 1
                }
            ),
            None
        );
    }
}
