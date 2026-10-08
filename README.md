# Glance

> 选中文本，译文自现。

系统级划词翻译工具。在任意应用中选中文本，译文自动浮现在鼠标旁——无需复制、无需切换窗口。

## 特性

- 零切换：选中即译，视线不离开
- 低打扰：延迟触发、智能抑制、一键关闭
- 三种触发：自动、半自动、快捷键
- 可配置：浮层布局、主题、按钮均可自定义
- 多引擎：DeepL、Google、OpenAI 等，支持自定义 API 与本地模型
- 隐私优先：本地缓存、API Key 系统加密、敏感应用自动禁用

## 平台

macOS / Windows 首发，Linux 与浏览器扩展规划中。

## 技术栈

Tauri 2 + Rust + Vue 3 + TypeScript + Tailwind CSS + SQLite

## 快速开始

```bash
# 安装依赖
cargo install tauri-cli --version "^2.0.0"
cargo install create-tauri-app --locked

# 克隆并运行
git clone https://github.com/your-org/glance.git
cd glance
pnpm install
pnpm tauri dev
```

## 打包

```bash
pnpm tauri build
```

## 结构

```
glance/
├── src-tauri/      # Rust 核心：取词、窗口、翻译引擎、存储
├── src/            # 前端：浮层、设置、引导
└── docs/           # 文档
```

---

**Glance — 一瞥即译。**