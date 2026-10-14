//! 选区防抖状态机。
//!
//! 输入 raw-input 事件，输出 [`Effect`]：启动防抖定时器或执行取词；
//! 定时器超时后以 `DebounceFired { generation }` 回送，代数不匹配的迟到事件直接丢弃。

use crate::selection::config::SelectionConfig;
use crate::selection::event::{Key, SelectionInputEvent};

/// 防抖状态机的状态
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionState {
    /// 空闲
    Idle,
    /// 选择中：鼠标按下拖拽，或 Shift+方向键扩展选区
    Selecting {
        /// 本次选择起始坐标（键盘选区无坐标，记为 (0, 0)）
        start_pos: (i32, i32),
        /// 位移是否已达到拖拽阈值
        is_dragging: bool,
    },
    /// 防抖等待中：等待定时器超时
    PendingConfirm { generation: u64 },
}

/// 状态机输出：交给 Engine 执行的效果
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effect {
    /// 忽略该事件
    None,
    /// 启动防抖定时器：超时后回送 `DebounceFired { generation }`
    ArmDebounce { generation: u64, delay_ms: u32 },
    /// 立即执行取词
    TriggerCapture,
}

/// 选区防抖状态机，支持鼠标与键盘（Shift+方向键）两类选区。
#[derive(Debug)]
pub struct SelectionStateMachine {
    state: SelectionState,
    /// 防抖代数：每次进入 PendingConfirm 递增
    generation: u64,
    /// 鼠标左键当前是否按住
    mouse_down: bool,
    /// Shift 当前是否按住
    shift_held: bool,
    /// 当前选区是否由 Shift+方向键 产生
    keyboard_selecting: bool,
}

impl SelectionStateMachine {
    pub fn new() -> Self {
        Self::default()
    }

    /// 当前状态
    pub fn state(&self) -> SelectionState {
        self.state
    }

    /// 当前防抖代数
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// 强制复位到 Idle
    pub fn reset(&mut self) {
        self.state = SelectionState::Idle;
        self.mouse_down = false;
        self.shift_held = false;
        self.keyboard_selecting = false;
    }

    /// 状态机主入口：输入 raw-input 事件，返回应执行的效果。
    pub fn handle(&mut self, event: SelectionInputEvent, config: &SelectionConfig) -> Effect {
        if !config.enabled {
            self.reset();
            return Effect::None;
        }

        let state = self.state;
        match state {
            SelectionState::Idle => self.handle_idle(event),
            SelectionState::Selecting { .. } => self.handle_selecting(event, config),
            SelectionState::PendingConfirm { generation } => self.handle_pending(event, generation),
        }
    }

    /// Idle：等待选择开始。各事件的处理：
    /// - 鼠标按下：进入 Selecting，记录起始坐标；
    /// - 鼠标移动：忽略；
    /// - 鼠标抬起（迟到的）：仅同步按键状态；
    /// - Shift+方向键：进入键盘选区；
    /// - Shift 按下 / 释放：仅跟踪修饰键状态；
    /// - Ctrl/Cmd+C：立即取词；
    /// - DebounceFired（迟到的定时器）：丢弃。
    fn handle_idle(&mut self, event: SelectionInputEvent) -> Effect {
        match event {
            SelectionInputEvent::MouseDown { pos } => {
                self.begin_mouse_selecting(pos);
                Effect::None
            }
            SelectionInputEvent::MouseMove { .. } => Effect::None,
            // 迟到的 MouseUp：仅同步按键跟踪
            SelectionInputEvent::MouseUp { .. } => {
                self.mouse_down = false;
                Effect::None
            }
            SelectionInputEvent::KeyDown { key, shift, ctrl } => match key {
                // Shift+方向键：键盘选区开始
                key if is_arrow(key) && shift => {
                    self.begin_keyboard_selecting();
                    Effect::None
                }
                Key::Shift => {
                    self.shift_held = true;
                    Effect::None
                }
                // Ctrl/Cmd+C：显式取词
                Key::C if ctrl => self.trigger_capture(),
                _ => Effect::None,
            },
            SelectionInputEvent::KeyUp { key, .. } => {
                if key == Key::Shift {
                    self.shift_held = false;
                }
                Effect::None
            }
            // 迟到的定时器：丢弃
            SelectionInputEvent::DebounceFired { .. } => Effect::None,
        }
    }

    /// Selecting：选择进行中（鼠标拖拽或键盘扩展）。各事件的处理：
    /// - 鼠标按下：未收到抬起就再次按下，重新开始一次选择；
    /// - 鼠标移动：累计位移达到 `drag_threshold_px` 后标记为拖拽；
    /// - 鼠标抬起：选择结束，进入 PendingConfirm 防抖
    ///   （单击未拖拽同样进入，双击选词依赖此路径）；
    /// - Shift+方向键：继续扩展选区；
    /// - 方向键（未按 Shift）：选区折叠，复位到 Idle；
    /// - Ctrl/Cmd+C：立即取词；
    /// - 其他按键：忽略，保持 Selecting 等待鼠标抬起；
    /// - Shift 释放：键盘选区（非鼠标拖拽中）视为选择结束，进入防抖；
    /// - DebounceFired：丢弃。
    fn handle_selecting(&mut self, event: SelectionInputEvent, config: &SelectionConfig) -> Effect {
        let SelectionState::Selecting {
            start_pos,
            is_dragging,
        } = self.state
        else {
            return Effect::None;
        };

        match event {
            // 未收到 MouseUp 就再次 MouseDown：重新开始一次选择
            SelectionInputEvent::MouseDown { pos } => {
                self.begin_mouse_selecting(pos);
                Effect::None
            }
            SelectionInputEvent::MouseMove { pos } => {
                // 位移达到阈值即视为拖拽选择
                let dx = i64::from(pos.0 - start_pos.0);
                let dy = i64::from(pos.1 - start_pos.1);
                let threshold = i64::from(config.drag_threshold_px);
                let dragging = is_dragging || dx * dx + dy * dy >= threshold * threshold;
                self.state = SelectionState::Selecting {
                    start_pos,
                    is_dragging: dragging,
                };
                Effect::None
            }
            // 抬起：选择结束，进入防抖等待
            SelectionInputEvent::MouseUp { .. } => {
                self.mouse_down = false;
                self.arm_debounce(config)
            }
            SelectionInputEvent::KeyDown { key, shift, ctrl } => match key {
                // Shift+方向键：继续扩展选区
                key if is_arrow(key) && shift => {
                    self.shift_held = true;
                    self.keyboard_selecting = true;
                    Effect::None
                }
                // 方向键（未按 Shift）：选区折叠，取消本次选择
                key if is_arrow(key) => {
                    self.reset();
                    Effect::None
                }
                Key::Shift => {
                    self.shift_held = true;
                    Effect::None
                }
                Key::C if ctrl => self.trigger_capture(),
                // 打字等其他按键：不改变拖拽中的选择，保持 Selecting 等 MouseUp
                _ => Effect::None,
            },
            SelectionInputEvent::KeyUp { key, .. } => match key {
                Key::Shift => {
                    self.shift_held = false;
                    // 键盘选区以 Shift 释放结束
                    if self.keyboard_selecting && !self.mouse_down {
                        self.arm_debounce(config)
                    } else {
                        Effect::None
                    }
                }
                _ => Effect::None,
            },
            SelectionInputEvent::DebounceFired { .. } => Effect::None,
        }
    }

    /// PendingConfirm：防抖等待中，等待定时器确认选区稳定。各事件的处理：
    /// - DebounceFired 且 generation 匹配：选区稳定，执行取词并复位；
    /// - DebounceFired 但 generation 不匹配：旧定时器，丢弃；
    /// - 鼠标按下：开始新的选择，取消计时，回到 Selecting；
    /// - 鼠标移动 / 抬起：不改变选区，忽略；
    /// - Shift+方向键：扩展选区，取消计时，回到 Selecting；
    /// - 方向键（未按 Shift）：选区折叠，复位到 Idle；
    /// - Ctrl/Cmd+C：立即取词；
    /// - 其他无 Ctrl 按键：打字将替换选区，复位到 Idle；
    /// - Ctrl 组合键：不动选区，保持等待；
    /// - Shift 释放：仅同步修饰键状态。
    fn handle_pending(&mut self, event: SelectionInputEvent, generation: u64) -> Effect {
        match event {
            // 定时器到点且 generation 匹配：选区稳定，执行取词
            SelectionInputEvent::DebounceFired { generation: g } if g == generation => {
                self.trigger_capture()
            }
            // 旧定时器：代数不匹配，丢弃
            SelectionInputEvent::DebounceFired { .. } => Effect::None,
            // 开始新的选择：取消计时
            SelectionInputEvent::MouseDown { pos } => {
                self.begin_mouse_selecting(pos);
                Effect::None
            }
            // 鼠标移动不改变选区，忽略
            SelectionInputEvent::MouseMove { .. } => Effect::None,
            SelectionInputEvent::MouseUp { .. } => Effect::None,
            SelectionInputEvent::KeyDown { key, shift, ctrl } => match key {
                // Shift+方向键：继续扩展选区，取消计时
                key if is_arrow(key) && shift => {
                    self.begin_keyboard_selecting();
                    Effect::None
                }
                // 方向键（未按 Shift）：选区折叠，取消
                key if is_arrow(key) => {
                    self.reset();
                    Effect::None
                }
                Key::Shift => {
                    self.shift_held = true;
                    Effect::None
                }
                Key::C if ctrl => self.trigger_capture(),
                // 打字：选区将被替换，取消；Ctrl 组合键不动选区，保持等待
                _ if !ctrl => {
                    self.reset();
                    Effect::None
                }
                _ => Effect::None,
            },
            SelectionInputEvent::KeyUp { key, .. } => {
                if key == Key::Shift {
                    self.shift_held = false;
                }
                Effect::None
            }
        }
    }

    /// 开始鼠标选择
    fn begin_mouse_selecting(&mut self, pos: (i32, i32)) {
        self.state = SelectionState::Selecting {
            start_pos: pos,
            is_dragging: false,
        };
        self.mouse_down = true;
        self.keyboard_selecting = false;
    }

    /// 开始键盘选择
    fn begin_keyboard_selecting(&mut self) {
        self.state = SelectionState::Selecting {
            start_pos: (0, 0),
            is_dragging: false,
        };
        self.shift_held = true;
        self.keyboard_selecting = true;
    }

    /// 进入 PendingConfirm 并产出 ArmDebounce
    fn arm_debounce(&mut self, config: &SelectionConfig) -> Effect {
        self.generation += 1;
        let generation = self.generation;
        self.state = SelectionState::PendingConfirm { generation };
        self.mouse_down = false;
        self.keyboard_selecting = false;
        Effect::ArmDebounce {
            generation,
            delay_ms: config.trigger_delay_ms,
        }
    }

    /// 执行取词并复位，使未决的防抖定时器失效
    fn trigger_capture(&mut self) -> Effect {
        self.reset();
        Effect::TriggerCapture
    }
}

impl Default for SelectionStateMachine {
    fn default() -> Self {
        Self {
            state: SelectionState::Idle,
            generation: 0,
            mouse_down: false,
            shift_held: false,
            keyboard_selecting: false,
        }
    }
}

fn is_arrow(key: Key) -> bool {
    matches!(
        key,
        Key::ArrowLeft | Key::ArrowRight | Key::ArrowUp | Key::ArrowDown
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> SelectionConfig {
        SelectionConfig::default()
    }

    fn down(x: i32, y: i32) -> SelectionInputEvent {
        SelectionInputEvent::MouseDown { pos: (x, y) }
    }

    fn mv(x: i32, y: i32) -> SelectionInputEvent {
        SelectionInputEvent::MouseMove { pos: (x, y) }
    }

    fn up(x: i32, y: i32) -> SelectionInputEvent {
        SelectionInputEvent::MouseUp { pos: (x, y) }
    }

    fn kd(key: Key, shift: bool, ctrl: bool) -> SelectionInputEvent {
        SelectionInputEvent::KeyDown { key, shift, ctrl }
    }

    fn ku(key: Key, shift: bool) -> SelectionInputEvent {
        SelectionInputEvent::KeyUp {
            key,
            shift,
            ctrl: false,
        }
    }

    fn fired(gen: u64) -> SelectionInputEvent {
        SelectionInputEvent::DebounceFired { generation: gen }
    }

    #[test]
    fn mouse_drag_selection_triggers_capture() {
        let mut sm = SelectionStateMachine::new();
        let cfg = config();

        assert_eq!(sm.handle(down(10, 10), &cfg), Effect::None);
        assert_eq!(
            sm.state(),
            SelectionState::Selecting {
                start_pos: (10, 10),
                is_dragging: false
            }
        );

        // 位移 (10, 2) ≥ 阈值 8px → 拖拽选择
        assert_eq!(sm.handle(mv(20, 12), &cfg), Effect::None);
        assert_eq!(
            sm.state(),
            SelectionState::Selecting {
                start_pos: (10, 10),
                is_dragging: true
            }
        );

        // 抬起 → 防抖，延迟取自配置
        assert_eq!(
            sm.handle(up(20, 12), &cfg),
            Effect::ArmDebounce {
                generation: 1,
                delay_ms: 300
            }
        );
        assert_eq!(sm.state(), SelectionState::PendingConfirm { generation: 1 });

        // 定时器到点且代数匹配 → 取词，回到 Idle
        assert_eq!(sm.handle(fired(1), &cfg), Effect::TriggerCapture);
        assert_eq!(sm.state(), SelectionState::Idle);
    }

    #[test]
    fn small_move_is_click_and_click_still_arms() {
        let mut sm = SelectionStateMachine::new();
        let cfg = config();

        sm.handle(down(10, 10), &cfg);
        // 位移 (2, 1) < 阈值 → 视为点击，不标记拖拽
        sm.handle(mv(12, 11), &cfg);
        assert_eq!(
            sm.state(),
            SelectionState::Selecting {
                start_pos: (10, 10),
                is_dragging: false
            }
        );
        // 单击也进入防抖（双击选词依赖此路径）
        assert_eq!(
            sm.handle(up(12, 11), &cfg),
            Effect::ArmDebounce {
                generation: 1,
                delay_ms: 300
            }
        );
    }

    #[test]
    fn mousemove_ignored_when_idle() {
        let mut sm = SelectionStateMachine::new();
        let cfg = config();

        assert_eq!(sm.handle(mv(999, 999), &cfg), Effect::None);
        assert_eq!(sm.state(), SelectionState::Idle);
    }

    #[test]
    fn mousemove_during_pending_does_not_reset_timer() {
        let mut sm = SelectionStateMachine::new();
        let cfg = config();

        sm.handle(down(0, 0), &cfg);
        sm.handle(up(0, 0), &cfg);
        // 抬手后的鼠标抖动不改变选区，不应重置计时
        assert_eq!(sm.handle(mv(1000, 1000), &cfg), Effect::None);
        assert_eq!(sm.handle(fired(1), &cfg), Effect::TriggerCapture);
    }

    #[test]
    fn reselection_cancels_pending() {
        let mut sm = SelectionStateMachine::new();
        let cfg = config();

        // 第一次按下抬起 → 防抖 g1
        sm.handle(down(5, 5), &cfg);
        sm.handle(up(5, 5), &cfg);
        // 立刻再次按下（双击）：取消 g1，重新进入 Selecting
        assert_eq!(sm.handle(down(5, 5), &cfg), Effect::None);
        assert_eq!(sm.handle(fired(1), &cfg), Effect::None);
        // 第二次抬起 → 防抖 g2，确认后触发
        assert_eq!(
            sm.handle(up(5, 5), &cfg),
            Effect::ArmDebounce {
                generation: 2,
                delay_ms: 300
            }
        );
        assert_eq!(sm.handle(fired(2), &cfg), Effect::TriggerCapture);
    }

    #[test]
    fn stale_generation_dropped() {
        let mut sm = SelectionStateMachine::new();
        let cfg = config();

        sm.handle(down(0, 0), &cfg);
        sm.handle(up(0, 0), &cfg); // g1
        sm.handle(down(0, 0), &cfg);
        sm.handle(up(0, 0), &cfg); // g2
                                   // 旧定时器醒来：代数不匹配，丢弃
        assert_eq!(sm.handle(fired(1), &cfg), Effect::None);
        assert_eq!(sm.handle(fired(2), &cfg), Effect::TriggerCapture);
    }

    #[test]
    fn ctrl_c_triggers_capture_from_idle() {
        let mut sm = SelectionStateMachine::new();
        let cfg = config();

        assert_eq!(
            sm.handle(kd(Key::C, false, true), &cfg),
            Effect::TriggerCapture
        );
        assert_eq!(sm.state(), SelectionState::Idle);
    }

    #[test]
    fn ctrl_c_triggers_and_cancels_pending() {
        let mut sm = SelectionStateMachine::new();
        let cfg = config();

        sm.handle(down(0, 0), &cfg);
        sm.handle(up(0, 0), &cfg); // g1 待确认
                                   // 显式取词意图：立即取词，且防抖定时器被取消
        assert_eq!(
            sm.handle(kd(Key::C, false, true), &cfg),
            Effect::TriggerCapture
        );
        assert_eq!(sm.handle(fired(1), &cfg), Effect::None);
        assert_eq!(sm.state(), SelectionState::Idle);
    }

    #[test]
    fn keyboard_selection_flow() {
        let mut sm = SelectionStateMachine::new();
        let cfg = config();

        // Shift+方向键：键盘选区开始
        assert_eq!(
            sm.handle(kd(Key::ArrowRight, true, false), &cfg),
            Effect::None
        );
        assert_eq!(
            sm.state(),
            SelectionState::Selecting {
                start_pos: (0, 0),
                is_dragging: false
            }
        );
        // 继续扩展
        assert_eq!(
            sm.handle(kd(Key::ArrowRight, true, false), &cfg),
            Effect::None
        );
        // Shift 释放 → 防抖
        assert_eq!(
            sm.handle(ku(Key::Shift, false), &cfg),
            Effect::ArmDebounce {
                generation: 1,
                delay_ms: 300
            }
        );
        assert_eq!(sm.handle(fired(1), &cfg), Effect::TriggerCapture);
    }

    #[test]
    fn lone_shift_tap_does_not_select() {
        let mut sm = SelectionStateMachine::new();
        let cfg = config();

        // 打大写字母时的 Shift 点按不应进入 Selecting、不应触发防抖
        assert_eq!(sm.handle(kd(Key::Shift, true, false), &cfg), Effect::None);
        assert_eq!(sm.state(), SelectionState::Idle);
        assert_eq!(sm.handle(ku(Key::Shift, false), &cfg), Effect::None);
        assert_eq!(sm.state(), SelectionState::Idle);
    }

    #[test]
    fn plain_arrow_cancels_keyboard_selecting() {
        let mut sm = SelectionStateMachine::new();
        let cfg = config();

        sm.handle(kd(Key::ArrowRight, true, false), &cfg);
        // 未配合 Shift 的方向键：光标移动，选区折叠
        assert_eq!(
            sm.handle(kd(Key::ArrowLeft, false, false), &cfg),
            Effect::None
        );
        assert_eq!(sm.state(), SelectionState::Idle);
        assert_eq!(sm.handle(ku(Key::Shift, false), &cfg), Effect::None);
    }

    #[test]
    fn pending_arrow_with_shift_returns_to_selecting() {
        let mut sm = SelectionStateMachine::new();
        let cfg = config();

        sm.handle(down(0, 0), &cfg);
        sm.handle(up(0, 0), &cfg); // g1
                                   // 防抖期间继续用键盘扩展选区：取消 g1，回到 Selecting
        assert_eq!(sm.handle(kd(Key::ArrowUp, true, false), &cfg), Effect::None);
        assert_eq!(
            sm.state(),
            SelectionState::Selecting {
                start_pos: (0, 0),
                is_dragging: false
            }
        );
        assert_eq!(
            sm.handle(ku(Key::Shift, false), &cfg),
            Effect::ArmDebounce {
                generation: 2,
                delay_ms: 300
            }
        );
        assert_eq!(sm.handle(fired(1), &cfg), Effect::None);
        assert_eq!(sm.handle(fired(2), &cfg), Effect::TriggerCapture);
    }

    #[test]
    fn typing_cancels_pending() {
        let mut sm = SelectionStateMachine::new();
        let cfg = config();

        sm.handle(down(0, 0), &cfg);
        sm.handle(up(0, 0), &cfg); // g1
                                   // 防抖期间打字：选区将被替换，取消触发
        assert_eq!(sm.handle(kd(Key::Other, false, false), &cfg), Effect::None);
        assert_eq!(sm.state(), SelectionState::Idle);
        assert_eq!(sm.handle(fired(1), &cfg), Effect::None);
    }

    #[test]
    fn disabled_ignores_all_events() {
        let mut sm = SelectionStateMachine::new();
        let on = config();
        let off = SelectionConfig {
            enabled: false,
            ..Default::default()
        };

        // 停用状态下一切事件被忽略
        assert_eq!(sm.handle(down(0, 0), &off), Effect::None);
        assert_eq!(sm.handle(kd(Key::C, false, true), &off), Effect::None);
        assert_eq!(sm.state(), SelectionState::Idle);

        // 防抖中途停用：定时器醒来后也被忽略
        sm.handle(down(0, 0), &on);
        sm.handle(up(0, 0), &on);
        assert_eq!(sm.handle(fired(1), &off), Effect::None);
        assert_eq!(sm.state(), SelectionState::Idle);
    }

    #[test]
    fn custom_delay_passed_through() {
        let mut sm = SelectionStateMachine::new();
        let cfg = SelectionConfig {
            trigger_delay_ms: 650,
            ..Default::default()
        };

        sm.handle(down(0, 0), &cfg);
        assert_eq!(
            sm.handle(up(0, 0), &cfg),
            Effect::ArmDebounce {
                generation: 1,
                delay_ms: 650
            }
        );
    }

    #[test]
    fn keyup_shift_in_idle_ignored() {
        let mut sm = SelectionStateMachine::new();
        let cfg = config();

        // 从未进入过 Selecting 的 Shift 释放不应触发防抖
        assert_eq!(sm.handle(ku(Key::Shift, false), &cfg), Effect::None);
        assert_eq!(sm.state(), SelectionState::Idle);
    }

    #[test]
    fn reset_clears_pending() {
        let mut sm = SelectionStateMachine::new();
        let cfg = config();

        sm.handle(down(0, 0), &cfg);
        sm.handle(up(0, 0), &cfg);
        sm.reset();
        assert_eq!(sm.state(), SelectionState::Idle);
        assert_eq!(sm.handle(fired(1), &cfg), Effect::None);
    }
}
