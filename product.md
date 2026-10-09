# SelectLens 划词翻译 · AI 开发指导文档

> **文档用途**：用于指导 AI 进行产品开发，包含产品逻辑、交互规则、技术架构与核心实现思路。
> **范围**：只包含产品逻辑、功能规则、状态机、交互行为、数据流、技术实现，**不包含任何 UI/界面/视觉相关内容**。
> **使用方式**：可作为 AI 编码助手的 System Prompt 或上下文，按模块分步实现。
> **前端框架**：Vue 3 + TypeScript + Pinia。


## 一、产品定义

**产品名**：SelectLens（划词译）
**一句话**：在任意操作系统中选中文本，自动弹出翻译结果；零切换、低打扰、可配置、可接自有 API。

**核心价值**
- 零切换：选中即译，不离开当前内容
- 低打扰：延迟触发、智能抑制、一键关闭
- 可配置：触发、引擎、API、应用规则均可调
- 隐私优先：本地缓存、Key 加密、敏感应用不触发
- 可扩展：翻译引擎插件化

**目标平台**：Windows / macOS（P0），Linux / 浏览器扩展（P1），移动端（P2）。

**目标用户**：外语阅读者、科研/论文用户、开发者、跨境电商、客服、外语学习者、企业内网用户。


## 二、产品原则

1. **不打扰优先**：自动弹出必须可延迟、可抑制、可全局关闭
2. **就近呈现**：结果出现在光标附近，不遮挡选中文本
3. **渐进披露**：默认简单，高级配置折叠
4. **状态可见**：翻译中、成功、失败、缓存、离线均有明确反馈
5. **可逆可退**：任何自动行为都能被关掉或改成手动
6. **一致性**：所有入口的行为逻辑统一


## 三、核心流程与状态机

### 3.1 主流程

```
Idle
→ 全局监听鼠标/键盘选中
→ 检测选区稳定 200–300ms
→ 抑制规则判断
→ 获取选中文本
→ 语言检测
→ 缓存查询
→ 翻译引擎路由
→ 流式/一次性返回
→ 结果呈现
→ 用户操作：复制/朗读/钉住/切引擎/生词本/追问
→ 关闭或钉住
```

### 3.2 结果状态机

| 状态 | 触发条件 | 行为 |
|---|---|---|
| `idle` | 无选区 | 不展示 |
| `loading` | 已触发翻译，等待首包 | 加载占位，操作能力禁用 |
| `streaming` | 引擎支持流式，正在返回 | 译文逐段追加，标记进行中 |
| `success` | 翻译完成 | 展示完整译文，操作能力可用 |
| `cached` | 命中本地缓存 | 展示译文，标识“缓存” |
| `error` | 翻译失败 | 显示错误原因 + 重试 + 切换引擎 |
| `empty` | 未识别到文本 | 提示重新选择 |
| `too-long` | 文本超过上限 | 提示已截断，提供“查看全文” |
| `pinned` | 用户点击钉住 | 结果锁定，不随鼠标移动，不自动关闭 |
| `no-permission` | 缺少辅助功能权限 | 提示去开启权限 |
| `offline` | 无网络且无缓存 | 提示离线，可切换本地模型 |


## 四、触发逻辑

### 4.1 触发模式

| 模式 | 行为 |
|---|---|
| 自动模式 | 选中文本后延迟指定时间，自动翻译并展示结果 |
| 半自动模式 | 选中后展示悬浮取词入口，用户主动确认才翻译 |
| 快捷键模式 | 选中后不自动翻译，按全局快捷键才翻译 |

默认：自动模式 + 智能抑制。

### 4.2 触发延迟

- 可配置范围：100–1000ms
- 默认：300ms
- 逻辑：选区稳定后开始计时，期间选区变化或取消则重置

### 4.3 抑制规则

以下情况不触发翻译：
- 密码框、支付页、隐私输入框
- 终端、命令行、全屏游戏、IDE 调试区
- 选中长度 < 1 字符
- 选中长度 > 5000 字符
- 输入法组合中
- 用户正在输入、拖动选择中
- 应用黑名单中的应用
- 未在白名单模式中且启用了白名单模式

### 4.4 应用规则

- 支持应用黑名单/白名单
- 支持按应用设置：触发模式、目标语言、引擎、是否启用
- 支持从“正在运行的应用”一键添加
- 应用规则优先于全局规则

### 4.5 其他触发方式

- 全局快捷键翻译
- 剪贴板翻译：复制文本后自动翻译
- 双击 Ctrl 翻译（可开关）
- OCR 划词（P1）
- 悬浮取词入口点击翻译


## 五、取词逻辑

### 5.1 取词优先级

1. 系统 Accessibility API 获取选中文本，不污染剪贴板
2. 回退模拟复制（Ctrl+C / Cmd+C），并保存与恢复剪贴板
3. 浏览器扩展使用 Selection API
4. 图片/PDF 使用 OCR

### 5.2 取词失败处理

- 提示“点击复制后翻译”
- 提供手动复制翻译入口
- 记录失败原因用于诊断

### 5.3 权限

- macOS：辅助功能权限
- Windows：UI Automation
- Linux：AT-SPI
- 未授权时：进入 `no-permission` 状态，提供“去开启”入口
- 首次运行流程中集中申请权限


## 六、翻译引擎与路由

### 6.1 引擎类型

- 内置：DeepL、Google、Microsoft、百度、有道
- 大模型：OpenAI、Claude、Gemini
- OpenAI 兼容接口
- 本地模型：Ollama、LM Studio
- 企业自建翻译服务、代理网关

### 6.2 路由策略

- 默认引擎
- 按语言对：中英用 A，英日用 B
- 按应用：IDE 用代码友好模型，论文用学术 Prompt
- 按优先级：速度 / 质量 / 成本
- 失败 Fallback：主引擎失败自动切备用
- 重试次数可配置
- BYOK：用户自带 Key，平台不承担成本

### 6.3 语言检测

- 自动检测源语言
- 检测失败时使用默认源语言
- 目标语言可全局设置，也可按应用覆盖

### 6.4 Prompt 模板

- 支持变量：`{{text}}`、`{{targetLang}}`、`{{sourceLang}}`、`{{context}}`
- 预设模板：学术、口语、代码注释、法律、电商
- 用户可自定义模板
- 模板按引擎/应用绑定


## 七、翻译结果的交互行为

> 本节只描述行为逻辑，不涉及展示形式。

### 7.1 出现与定位

- 触发后出现在光标附近
- 优先鼠标右侧，空间不足自动翻转左侧或上方
- 不得遮挡选中文本
- 多显示器按所在屏幕计算边界
- 全屏应用默认不展示（可配置）

### 7.2 关闭逻辑

以下操作关闭结果：
- 点击结果区域外部
- 按 `Esc`
- 页面滚动（钉住状态除外）
- 切换窗口
- 选区取消
- 用户手动关闭

### 7.3 操作能力

| 能力 | 行为 |
|---|---|
| 复制 | 复制译文，短暂反馈“已复制” |
| 朗读 | 播放 TTS，再次触发暂停 |
| 钉住 | 切换锁定状态，结果不再自动关闭 |
| 引擎切换 | 选择其他引擎，立即重新翻译 |
| 生词本 | 加入生词本，状态切换 |
| 追问 | 打开输入框，支持快捷指令 |
| 更多 | 展开更多操作，如替换原文、分享、反馈 |

### 7.4 钉住与多结果

- 钉住后结果可拖动，记忆位置
- 支持多结果并存，最多 3 个，层叠显示
- 每个结果独立操作

### 7.5 追问交互

- 输入框：“追问这段翻译…”
- 快捷指令：解释语法、更口语、更正式、举例句
- 返回结果替换译文区，提供“返回原文”
- 追问历史可查看

### 7.6 边界规则

- 文本过长：截断并提示，提供查看全文
- 无网络：查缓存 → 本地模型 → 离线提示
- 引擎失败：自动 Fallback，失败 2 次后展示错误
- 剪贴板回退：翻译后恢复原剪贴板内容


## 八、系统配置项规格

> 本节描述产品可配置的能力范围，不涉及配置界面。

### 8.1 通用配置

| 配置项 | 默认值 |
|---|---|
| 开机自启动 | 开 |
| 界面语言 | 简体中文 |
| 默认目标语言 | 中文（简体） |
| 默认源语言 | 自动检测 |
| 显示托盘入口 | 开 |
| 检查更新 | — |

### 8.2 触发配置

| 配置项 | 默认值 |
|---|---|
| 触发模式 | 自动 |
| 触发延迟 | 300ms |
| 全局快捷键 | Alt+T |
| 剪贴板翻译 | 关 |
| 双击 Ctrl 翻译 | 关 |
| OCR 划词 | 关（P1） |
| 应用黑名单 | 预设终端/密码管理器 |
| 应用白名单模式 | 关 |

### 8.3 外观配置能力

> 仅描述可配置能力，不涉及具体样式。

- 主题：浅色 / 深色 / 跟随系统 / 自定义
- 结果展示布局模式：原文+译文 / 仅译文 / 左右对照 / 逐句对照
- 字号、最大宽度、透明度、圆角、阴影强度
- 操作按钮显隐与排序
- 出现/关闭动画
- 自定义 CSS
- 配置导入/导出
- 主题市场（P2）
- 所有改动实时生效

### 8.4 引擎配置

- 默认引擎选择
- 引擎列表：状态、延迟、操作
- 每引擎可配置：目标语言映射、Prompt 模板、术语库绑定
- 路由策略：优先级、Fallback 引擎、失败重试次数

### 8.5 API 接入配置

| 字段 | 必填 |
|---|---|
| 名称 | 是 |
| 类型（OpenAI 兼容/DeepL/Google/百度/有道/自定义） | 是 |
| Base URL | 是 |
| API Key | 是 |
| 模型 | 否 |
| 自定义 Headers | 否 |
| 请求超时 | 否 |
| 限流 QPS | 否 |
| 代理 | 否 |
| Prompt 模板 | 否 |
| 响应字段映射 | 否 |

- 支持“测试连接”，返回延迟或错误码
- API Key 加密存储在系统钥匙串
- 企业策略开启时，外部云选项禁用

### 8.6 术语库

- 条目：原文 / 译文 / 大小写敏感 / 备注
- 支持批量导入 CSV/JSON
- 可绑定到引擎或应用
- 翻译命中术语时高亮，可查看备注

### 8.7 应用规则

- 按应用配置：触发模式、目标语言、引擎、是否启用
- 从“正在运行的应用”一键添加
- 支持搜索与排序
- 应用规则优先于全局规则

### 8.8 历史记录

- 记录：原文、译文、时间、来源应用、引擎
- 支持搜索、日期筛选、清空
- 支持复制、收藏、删除
- 保留时长：7天 / 30天 / 永久 / 不保存

### 8.9 生词本

- 条目：词条、译文、例句、标签、掌握度
- 支持复习模式（P2）
- 导出 Anki/CSV

### 8.10 隐私与安全配置

| 配置项 | 默认值 |
|---|---|
| 本地缓存 | 开 |
| 历史记录 | 开 |
| 发送匿名统计 | 关 |
| 敏感应用自动禁用 | 开 |
| 日志脱敏 | 开（锁定） |
| 清除所有本地数据 | — |

### 8.11 账号与同步（P2）

- 登录/注册
- 同步：设置、术语库、生词本、历史
- 冲突处理：以最新为准，可手动选择

### 8.12 关于与更新

- 版本号、更新日志
- 检查更新
- 开源许可
- 反馈入口


## 九、首次运行流程

| 步骤 | 逻辑 |
|---|---|
| 步骤 1 | 介绍产品价值，引导进入下一步 |
| 步骤 2 | 申请系统权限，未授权可跳过但提示功能受限 |
| 步骤 3 | 选择内置引擎或稍后配置 |
| 步骤 4 | 选择触发模式（自动/半自动/快捷键） |
| 步骤 5 | 内嵌示例文本，引导用户划词验证，成功提示“一切就绪” |

- 流程可跳过
- 配置中可重新运行


## 十、系统集成功能

### 10.1 托盘入口

- 启用/停用划词翻译
- 触发模式切换
- 目标语言切换
- 打开配置
- 生词本
- 检查更新
- 退出

### 10.2 快捷键

- 全局快捷键：手动翻译
- 快捷键录制：支持组合键、冲突检测
- 可恢复默认


## 十一、数据与缓存逻辑

- 本地缓存：翻译结果、语言检测结果
- 缓存命中：优先展示，标识“缓存”
- 缓存失效：文本变化、引擎变化、术语库更新
- 历史记录：本地存储，可关闭
- 生词本：本地存储，可同步（P2）
- 术语库：本地存储，可导入导出
- API Key：系统钥匙串加密
- 日志：脱敏，不记录原文（可配置）


## 十二、错误处理与边界

| 场景 | 处理 |
|---|---|
| 取词失败 | 提示点击复制后翻译 |
| 翻译失败 | 自动 Fallback，失败 2 次后显示错误 + 重试 |
| 无网络 | 查缓存 → 本地模型 → 离线提示 |
| 文本过长 | 截断并提示，提供查看全文 |
| 引擎未配置 | 提示去配置，提供快捷入口 |
| 权限缺失 | 提示去开启权限 |
| 剪贴板冲突 | 翻译后恢复原剪贴板 |
| 多显示器 | 按所在屏幕计算边界 |
| 全屏应用 | 默认不触发，可配置 |
| 输入法组合 | 不触发 |


## 十三、权限与隐私

- 辅助功能权限：macOS、Windows、Linux
- 网络权限：仅翻译请求
- 剪贴板权限：回退取词时使用
- 屏幕录制/OCR：仅 OCR 功能需要
- 所有数据默认本地
- 用户可完全关闭历史与缓存
- 敏感应用自动禁用
- 日志脱敏，不记录原文


## 十四、技术架构

### 14.1 整体分层

```
┌─────────────────────────────────────────────┐
│         Frontend (Vue 3 + TypeScript)        │
│              Tauri IPC                       │
├─────────────────────────────────────────────┤
│            Rust Core (Tauri Shell)           │
│  Selection Engine / Window Manager           │
│  Hotkey Manager / Provider Registry          │
│  State Store (SQLite + Keychain)             │
├─────────────────────────────────────────────┤
│           Platform Native Layer              │
│  macOS: AXAPI + NSPanel                      │
│  Windows: UIA + WPF                          │
│  Linux: AT-SPI + X11/Wayland                 │
└─────────────────────────────────────────────┘
```

### 14.2 技术选型

| 决策点 | 选择 | 理由 |
|---|---|---|
| 桌面框架 | Tauri 2 | 体积小、Rust 原生系统集成、多窗口成熟 |
| 取词方案 | Rust 原生 + 双通道（A11y + 剪贴板回退） | 性能好、跨平台、不污染剪贴板 |
| 独立窗口 | Tauri 独立 WebView 窗口 | 原生置顶/透明/无边框 |
| 翻译引擎 | Rust trait + 动态注册 | 插件化、类型安全、支持流式 |
| 流式输出 | Tauri event + SSE | 逐段推送 |
| API Key 存储 | 系统钥匙串（keyring crate） | OS 级加密 |
| 本地数据 | SQLite（rusqlite） | 轻量、无需额外进程 |
| 前端框架 | Vue 3 + Pinia + TypeScript | 生态成熟、组件复用、状态管理简洁 |

### 14.3 核心模块

```
src-tauri/
├── selection/              # 取词引擎
│   ├── engine.rs           # 主逻辑
│   ├── state.rs            # 状态机
│   ├── debounce.rs         # 防抖
│   ├── input_monitor.rs    # raw-input 监听
│   ├── platform/           # 平台取词实现
│   ├── clipboard.rs        # 剪贴板回退
│   └── suppress.rs         # 抑制规则
├── window/                 # 窗口管理
├── provider/               # 翻译引擎注册
├── store/                  # SQLite 数据层
├── hotkey/                 # 全局热键
└── commands/               # Tauri IPC 命令

src/
├── entries/                # 各独立窗口入口
├── shared/                 # 跨入口复用逻辑
│   ├── composables/
│   └── stores/             # Pinia stores
└── tauri/
    ├── commands.ts
    └── events.ts
```


## 十五、核心实现思路：取词引擎

### 15.1 整体架构

```
Input Monitor (raw-input 全局事件钩子)
    │ Event
    ▼
Selection State Machine (防抖状态机)
    │ Trigger
    ▼
Text Capture (取词执行)
    │ Result
    ▼
Tauri Event → 结果呈现
```

**设计原则**：事件监听与防抖判断在 Rust 侧独立完成，前端只接收最终的“取词结果”事件。

### 15.2 事件监听层

**推荐库**：`raw-input`（跨平台全局键盘/鼠标事件监听）

核心模块：
- `Core`：底层驱动引擎，管理 OS 事件循环
- `Listen`：事件监听接口，返回 `SubscriptionHandle`
- `Simulate`：输入模拟
- `Grab`：输入拦截

**需要监听的事件**：

| 事件 | 用途 |
|---|---|
| `MouseDown`（左键） | 标记选择开始 |
| `MouseMove` | 判断是否发生拖拽（位移 > 阈值） |
| `MouseUp`（左键） | 标记选择结束，启动防抖计时 |
| `KeyDown`（Shift + 方向键） | 键盘选区，标记选择中 |
| `KeyUp`（Shift 释放） | 键盘选区结束，启动防抖计时 |
| `KeyDown`（Ctrl+C / Cmd+C） | 手动复制，视为显式取词意图 |

**启动方式**：

```rust
thread::spawn(|| {
    Core::start().expect("Failed to start raw-input core");
});
Listen::start();
let handle = Listen::subscribe(|event| {
    let _ = tx.send(event); // 发送到 Selection Engine
});
```

### 15.3 防抖状态机

**核心逻辑**：300ms 延迟本质是防抖——用户完成选择后等待一段时间确认稳定，期间如有新事件则重置计时。

**状态转换**：

```
Idle
 │ MouseDown / KeyDown(Shift)
 ▼
Selecting ────────────────────────────┐
 │ MouseMove (位移 < drag_threshold)  │
 │ 视为点击，继续等待                  │
 │ MouseMove (位移 ≥ drag_threshold)  │
 │ 标记为拖拽选择                      │
 │ MouseUp / KeyUp(Shift)              │
 ▼                                    │
PendingConfirm (防抖等待 300ms)        │
 │ 新事件 → 取消计时，回 Idle 或 Selecting ─┘
 │ 300ms 超时且无新事件
 ▼
TriggerCapture → 执行取词
```

**关键实现：generation 取消机制**

```rust
enum SelectionState {
    Idle,
    Selecting { start_pos: (i32, i32), is_dragging: bool },
    PendingConfirm { generation: u64 },
}

impl SelectionEngine {
    async fn start_debounce(&mut self) {
        self.generation += 1;
        let gen = self.generation;
        self.state = SelectionState::PendingConfirm { generation: gen };

        let tx = self.tx.clone();
        let delay = self.config.trigger_delay_ms;

        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(delay)).await;
            let _ = tx.send(SelectionEvent::DebounceFired(gen)).await;
        });
    }
}
```

每次启动新计时器时递增 `generation`，旧计时器醒来后检查是否匹配，不匹配则丢弃。比 `select!` + `abort` 更简单且无竞态。

**拖拽阈值（drag_threshold_px）**：推荐 5–10px。
- 太低（< 3px）：鼠标微抖就触发，点击被误判为选择
- 太高（> 15px）：小幅拖拽选择短文本无法触发

**键盘选区支持**：`KeyDown` 且 `shift` 按下 → `Selecting`；`KeyUp` 且 `shift` 释放 → `PendingConfirm`。

### 15.4 取词重试与降级

```
尝试 1: AXAPI / UIA 直接读取
  │ 失败（返回空或超时）
  ▼
尝试 2: 等待 50ms 后重试（无障碍树可能尚未更新）
  │ 失败
  ▼
尝试 3: 剪贴板回退（模拟复制 + 读取 + 恢复）
  │ 失败
  ▼
返回 CaptureOutcome::Failed，携带诊断信息
```

```rust
async fn capture_text(&self) -> Result<SelectionData, CaptureError> {
    // 策略 1: 无障碍 API
    match self.platform.capture_via_a11y().await {
        Ok(text) if !text.is_empty() => return Ok(text),
        _ => {}
    }
    // 策略 2: 等待 50ms 重试
    tokio::time::sleep(Duration::from_millis(50)).await;
    match self.platform.capture_via_a11y().await {
        Ok(text) if !text.is_empty() => return Ok(text),
        _ => {}
    }
    // 策略 3: 剪贴板回退
    self.platform.capture_via_clipboard().await
}
```

### 15.5 线程模型

```
Main Thread (Tauri)
  │
  ├── Tokio Runtime
  │     ├── Selection Engine Task (事件处理 + 防抖)
  │     ├── Translation Task (翻译请求)
  │     └── Provider Registry Task
  │
  └── Platform Event Thread (raw-input 钩子线程)
        └── mpsc channel → Selection Engine
```

raw-input 在 Windows 上需要消息循环，钩子运行在独立线程，通过 channel 与 Tokio 通信。

### 15.6 性能优化

- **钩子回调中不做重活**：只做 `tx.send(event)`，逻辑在 Tokio 任务中异步处理
- **事件节流**：`MouseMove` 频率极高，只在 `Selecting` 状态下处理，`Idle` 状态直接丢弃
- **结果展示期间暂停监听**：使用 raw-input 的 `handle.pause()` 避免结果操作被误判为新选择

### 15.7 配置热更新

```rust
pub struct SelectionEngine {
    config: Arc<RwLock<SelectionConfig>>,
    state: SelectionState,
    generation: u64,
    tx: mpsc::Sender<SelectionEvent>,
    rx: mpsc::Receiver<SelectionEvent>,
}
```

修改 `trigger_delay_ms` 或 `drag_threshold_px` 时热更新，无需重启引擎。

### 15.8 macOS 权限检查

```rust
#[cfg(target_os = "macos")]
fn check_accessibility_permission() -> bool {
    unsafe {
        let options = CFDictionaryCreate(/* ... */);
        AXIsProcessTrustedWithOptions(options)
    }
}
```

未授权时 raw-input 事件回调不会被触发，需进入 `no-permission` 状态。

### 15.9 推荐复用的开源库

| 库 | 可复用部分 |
|---|---|
| `raw-input` | 全局事件监听 + 订阅管理 |
| `monio` | 拖拽检测（区分 MouseDragged / MouseMoved） |
| `selection-capture` | 取词策略 fallback + 重试 + 取消 |
| `on-selected-text` | `DaemonOptions` 配置（drag_threshold_px + trigger_delay_ms） |
| `selection-hook` | 全平台事件监听 + 选区监听完整参考 |

**建议**：使用 `raw-input` 做事件监听，参考 `on-selected-text` 配置模型和 `selection-capture` fallback 设计，自行实现防抖状态机。


## 十六、开发路线图

| 阶段 | 内容 | 预估周期 |
|---|---|---|
| P0 骨架 | Tauri 初始化、多窗口框架、SQLite 集成、Selection Engine 骨架 | 2–3 周 |
| P0 核心 | macOS + Windows 取词、结果呈现、1 个内置 Provider | 3–4 周 |
| P0 配置 | 触发/引擎/API 配置、系统钥匙串集成 | 2–3 周 |
| P1 增强 | 流式输出、多 Provider、术语库、生词本、应用规则 | 3–4 周 |
| P1 完善 | 首次运行流程、托盘入口、历史记录、隐私配置 | 2–3 周 |
| P2 | OCR 划词、Linux 支持、同步、主题市场 | 按需 |

**技术风险**：
1. **取词兼容性**是最大风险——部分 Electron、Java 应用、游戏全屏可能无法通过 A11y API 取词，必须实现可靠的剪贴板回退
2. **macOS 辅助功能权限**是硬性门槛，需在首次运行流程中清晰引导授权
3. **流式输出窗口间通信**需注意 Tauri event 性能，高频 chunk 可能需批量合并（如每 50ms 推送累积文本）


## 十七、给 AI 开发的执行建议

### 17.1 按模块分步实现

建议 AI 按以下顺序开发：

1. **项目骨架**：Tauri 2 + Vue 3 + TypeScript 初始化，多窗口配置
2. **数据层**：SQLite schema 设计、配置读写、keyring 集成
3. **Selection Engine**：raw-input 事件监听 + 防抖状态机 + 取词 fallback
4. **Provider 系统**：trait 定义 + 内置 Provider 实现 + 路由逻辑
5. **结果呈现**：状态机渲染 + 操作能力 + 定位逻辑
6. **配置系统**：所有配置项的读写与热更新
7. **首次运行流程**：5 步流程 + 权限申请
8. **托盘与快捷键**：系统集成
9. **历史/生词本/术语库**：数据展示 + 导入导出
10. **隐私与安全**：日志脱敏 + 数据清除

### 17.2 关键约束

- 渲染进程永远不接触明文 API Key
- 取词失败必须有降级方案
- 所有自动化行为可被用户关闭
- 结果不得遮挡选中文本
- 敏感应用默认不触发

### 17.3 验收标准

- 自动模式选中文本后 300ms 内出现结果
- 结果位置不遮挡选中文本，空间不足自动翻转
- 取词成功率 > 95%（主流应用）
- API Key 加密存储，日志无明文
- 所有配置项修改后即时生效或明确提示需重启
- 首次运行流程可跳过，配置中可重新运行