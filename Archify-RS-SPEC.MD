Archify-RS 技术规范文档 (最终版)
版本: v1.0
状态: 可交付
目标: 使用 Rust 构建一个完全替代 Node.js/npm 工具链的 Archify CLI，保留 100% 前端渲染能力，并可作为独立 Skill 集成。

1. 项目背景
Archify 是一个强大的架构可视化工具，能从代码仓库或系统描述生成精美的交互式 HTML 图表。但其工具链依赖 Node.js 和 npm，增加了使用门槛和依赖负担。

Archify-RS 旨在保留 Archify 的全部核心价值（尤其是其强大的前端渲染器和交互体验），同时将构建、校验、转换等后端逻辑用 Rust 原生重写，实现：

零依赖：单文件二进制，无需 Node.js/npm

极速启动：毫秒级响应

跨平台：Windows/Linux/macOS 原生

无缝集成：可作为 AI Agent Skill 直接调用

2. 核心原则（不可违背）
🔒 前端渲染器 100% 保留
原 template.html 中的所有 CSS、JavaScript、交互逻辑（拖拽、缩放、主题切换、导出等）一字不改。

Rust CLI 绝不修改或重写任何前端代码，仅负责将 JSON IR 注入 __ARCHIFY_DATA__ 占位符。

后续 Archify 前端升级，只需替换 assets/template.html 即可自动获得新能力。

🧩 完全兼容现有数据格式
输入 JSON IR 完全兼容 Archify 官方示例（examples/*.architecture.json）。

支持 Mermaid 文本转换（sequenceDiagram, flowchart 等）。

输出 HTML 与原始 Archify 生成的视觉上像素级一致。

3. 命令行接口 (CLI)
3.1 全局选项
bash
archify-rs [OPTIONS] <COMMAND>
选项	说明
-v, --verbose	输出调试信息
-c, --config <FILE>	指定配置文件（可选）
-h, --help	显示帮助
3.2 子命令
A. validate — 校验 JSON IR
bash
archify-rs validate -t <TYPE> -i <INPUT.json>
-t, --type: 图表类型（architecture, workflow, sequence, dataflow, lifecycle）

-i, --input: 输入 JSON 文件路径

返回：校验通过（退出码 0）或详细错误信息

B. render — 生成 HTML 图表
bash
archify-rs render -t <TYPE> -i <INPUT.json> -o <OUTPUT.html>
-i, --input: 输入的 JSON 文件

-o, --output: 输出的 HTML 路径（默认 ./archify-output.html）

--theme [dark\|light]: 可选指定默认主题

核心：将 JSON 数据注入到嵌入的 template.html 的 __ARCHIFY_DATA__ 位置

C. convert — Mermaid → JSON IR
bash
archify-rs convert -f <input.mmd> -t <TYPE> -o <IR.json>
-f, --file: Mermaid 文本文件

将 flowchart 或 sequenceDiagram 转换为相应的 JSON IR

D. analyze — 从代码仓库生成架构 IR（扩展）
bash
archify-rs analyze --path <REPO_DIR> -o <IR.json>
--path: 代码仓库根目录

--lang <LANG>: 指定编程语言（python, rust, typescript 等），默认自动检测

利用 tree-sitter 解析源码，提取组件与依赖关系

4. 技术栈与架构
4.1 Rust Crates 选型
功能	Crate	理由
CLI 解析	clap (derive)	强大且轻量，支持子命令
JSON 处理	serde_json	事实标准，零拷贝解析
Schema 校验	jsonschema	支持 Draft 7/2019-09
模板引擎	handlebars 或 tera	但本项目中 仅做字符串替换，无需复杂模板
Mermaid 解析	pest / nom	构建健壮的解析器
代码分析	tree-sitter (绑定)	多语言 AST 解析
文件遍历	walkdir	高效递归
错误处理	anyhow + thiserror	优雅错误上下文
日志	env_logger	调试输出
4.2 数据流管道
text
+------------+      +-----------------+      +-----------------+      +-----------+
| 输入源      | ---> | 解析/转换/校验   | ---> | 标准化 JSON IR  | ---> | 渲染/输出  |
| (JSON/     |      | (Validator,      |      | (统一数据结构)   |      | (HTML文件) |
|  Mermaid/  |      |  Converter,      |      |                 |      |           |
|  Repo)     |      |  Analyzer)       |      |                 |      |           |
+------------+      +-----------------+      +-----------------+      +-----------+
4.3 核心数据结构（对应 Archify Schema）
rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct IR {
    pub schema_version: u8,
    pub diagram_type: String,
    pub title: String,
    pub components: Vec<Component>,
    pub connections: Vec<Connection>,
    pub boundaries: Option<Vec<Boundary>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Component {
    pub id: String,
    pub name: String,
    pub kind: String,      // "service", "database", "external"
    pub layer: Option<String>,
    pub description: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Connection {
    pub from: String,
    pub to: String,
    pub label: Option<String>,
    pub style: Option<String>, // "solid", "dashed"
}
5. 模块实现细节
5.1 渲染器 (Renderer) —— 最小化职责
编译时：通过 include_str! 将 assets/template.html 硬编码进二进制。

运行时：

将输入的 IR 序列化为压缩的 JSON 字符串（serde_json::to_string）。
用该字符串替换模板中的 __ARCHIFY_DATA__ 占位符。
写入输出文件。
严格禁止：修改任何 HTML/CSS/JS 内容。

5.2 校验器 (Validator) —— 替代 ajv
将 Archify 官方 JSON Schema（从 schemas/ 目录嵌入）编译为 jsonschema 校验器。

缓存校验器实例（使用 lazy_static 或 once_cell），避免重复编译开销。

校验失败时，输出清晰的错误路径与期望值。

5.3 转换器 (Converter) —— Mermaid → IR
使用 pest 定义 Mermaid 语法（支持 flowchart 和 sequenceDiagram）。

映射规则：

每个节点 → Component

每条连线（箭头）→ Connection

自动根据节点位置推断 layer（如 frontend / backend）。

输出完整的 IR JSON。

5.4 分析器 (Analyzer) —— 代码仓库 → IR（可选）
使用 tree-sitter 解析代码文件，识别导入、导出、类、函数等。

构建依赖图，生成 Component（每个模块/文件）和 Connection（依赖关系）。

支持语言：Python, Rust, TypeScript/JavaScript, Go, Java（由 tree-sitter 支持）。

6. 构建与打包
6.1 编译配置
在 Cargo.toml 中启用优化：

toml
[profile.release]
lto = true          # 链接时优化
strip = true        # 移除符号
codegen-units = 1   # 更好的优化
6.2 静态资源嵌入
assets/template.html → 使用 include_str! 嵌入

schemas/*.json → 同样嵌入，用于校验

6.3 跨平台构建
使用 cross 或 GitHub Actions 构建各平台二进制：

x86_64-unknown-linux-gnu

x86_64-pc-windows-msvc

x86_64-apple-darwin

aarch64-apple-darwin (M1/M2)

6.4 发布
GitHub Releases 提供下载

Homebrew / Scoop 可增加安装脚本

7. Skill 集成
7.1 Skill 定义文件 (archify-rs.skill.md)
markdown
---
name: archify-rs
description: 从代码库、Mermaid 或 JSON 生成架构图（HTML），无需 Node.js。
tools: [archify-rs]
---

# 指令
当用户请求生成架构可视化时，使用 `archify-rs` 工具：

1. **若用户提供 Mermaid 文本**：保存为 `.mmd`，执行 `convert`。
2. **若用户指明确认代码仓库**：执行 `analyze`。
3. **若已有 JSON IR**：直接 `render`。
4. 最终打开生成的 HTML 文件。

## 安装指引
- 从 GitHub Releases 下载对应平台二进制，放置到 `PATH`。

## 注意事项
- 此工具不依赖 Node.js。
- 所有前端渲染能力与原始 Archify 一致。


8. 测试与验收
8.1 单元测试
每个模块（validator, converter, analyzer）的独立测试。

使用 examples/ 下的 JSON 作为测试数据。

8.2 快照测试（关键）
对比 原版 Archify 与 Archify-RS 生成的 HTML 文件，确保 DOM 结构和注入数据完全一致。

可使用 insta crate 进行快照管理。

8.3 功能测试
在主流浏览器中打开生成的 HTML，验证交互功能：

节点拖拽、缩放

主题切换

导出 PNG/SVG

搜索高亮

8.4 性能测试
冷启动时间 < 50ms

处理 500+ 节点 JSON 的渲染时间 < 2s

内存占用 < 20MB

9. 未来扩展 (Roadmap)
版本	新增特性
v1.1	支持 watch 模式，自动重渲染
v1.2	原生 SVG 导出（不依赖浏览器）

10. 贡献与维护
项目托管于 GitHub，接受 PR。

前端模板（template.html）应定期从官方 Archify 同步更新。

新功能优先在 Rust 后端实现，前端仅作为呈现层。

附录 A：文件结构
text
archify-rs/
├── Cargo.toml
├── build.rs                   # （可选）校验嵌入式模板
├── src/
│   ├── main.rs
│   ├── cli.rs                 # clap 命令定义
│   ├── validator.rs
│   ├── converter.rs
│   ├── analyzer.rs
│   ├── renderer.rs
│   └── ir.rs                  # 数据结构定义
├── assets/
│   └── template.html          # ⭐ 从官方 Archify 原样复制
├── schemas/
│   └── architecture.schema.json # ⭐ 官方 Schema
├── examples/                  # 测试数据
│   ├── archify-repo.architecture.json
│   └── ...
├── tests/
│   ├── snapshot_tests.rs
│   └── integration_tests.rs
└── README.md
附录 B：示例使用
bash
# 1. 校验
archify-rs validate -t architecture -i my-arch.json

# 2. 从 Mermaid 转换
archify-rs convert -f diagram.mmd -t workflow -o workflow.json

# 3. 渲染 HTML
archify-rs render -t architecture -i my-arch.json -o report.html

# 4. 分析代码仓库（需要 tree-sitter 支持）
archify-rs analyze --path ./my-service -o service.json
结语
Archify-RS 是 Archify 精神的 Rust 化实现，它既保留了原项目的精髓（出色的前端可视化能力），又消除了所有外部依赖，使其更轻量、更通用、更易于集成到现代 AI 开发工作流中。本规范文档是所有开发与维护工作的唯一依据，请严格遵循。

文档版本: 1.0
最后更新: 2026-08-08
维护者: Archify-RS 团队