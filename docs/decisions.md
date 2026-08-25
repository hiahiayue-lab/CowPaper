# CowPaper 关键决策记录（ADR）

本文记录对原始需求书（V1.0）的偏离与补充，作为后续开发的依据。原始需求书 §1/§3.2/§13/§18 中被覆盖的条目以本文件为准。

## D1：交付形态由「SwiftUI 原生」改为「Tauri 单 .app」

- **原需求**：§1、§3.2、§13.1 要求 SwiftUI 原生 macOS 应用 + SQLite/GRDB，明确排除网页版本。
- **新决策**：用户明确要求「HTML 工作台式界面 + 方便分发」。最终采用 **Tauri**（HTML/CSS/JS 前端 + Rust 本地内核），打包为单个 `.app`，朋友拖入 Applications 双击即用。
- **理由**：保留网页式工作台体验，同时获得单文件分发、原生钥匙串/后台任务能力、本地 SQLite，且无需自建服务器。
- **影响**：§13.1 的 SwiftUI/GRDB 改为「HTML 前端 + Rust + rusqlite(bundled SQLite)」；§3.2 中「不考虑网页版本」作废（界面本身是网页，但打包为原生桌面应用）。

## D2：DeepSeek Key 存储为浏览器明文（用户明确选择）

- **原需求**：§13.2、§18.15 要求 Key 不得明文。
- **新决策**：用户两次明确选择「浏览器明文存储」（localStorage）。
- **影响**：**§18.15 第 15 条「Key 不出现在明文配置文件」不满足**，已向用户说明并获接受。阶段三实现时 Key 存 localStorage，仅限本机自用；若日后要分享给他人需各自填 Key。
- **备注**：因采用 Tauri，后续如需升级为钥匙串存储成本极低（`tauri-plugin-stronghold` 或系统钥匙串），可作为后续增强。

## D3：数据源策略（阶段一验证结论）

- 8 本目标期刊全部可经 Crossref + OpenAlex 发现论文，无需 RSS、无需出版社适配器即可完成闭环。
- **发现顺序**：Crossref（主力，最及时）→ OpenAlex（补漏 + 补摘要）。RSS 仅 INFORMS 3 本可用，作为可选加速项（阶段四）。
- **摘要缺口**：JPE（UChicago）、JMIS（T&F）不向 Crossref 存摘要（0%），靠 OpenAlex 补齐（分别 ~59%、~98%）。JPE 约四成论文将处于 `waitingForAbstract`。
- **阶段五适配器**：仅 JPE 待评估；其余 7 本无需适配器。
- 详见 `docs/phase1-report.md`。

## D4：技术栈细节

- Rust 工具链安装在工作区 `.rustup`/`.cargo`（沙箱限制，用 `RUSTUP_HOME`/`CARGO_HOME` 重定向），crates.io 走 rsproxy 镜像（`.cargo/config.toml`）。
- 依赖：`rusqlite`(bundled SQLite)、`reqwest`(blocking + rustls)、`chrono`、`serde`、`tauri v2`。
- 网络：Crossref 需 `mailto`（polite pool）；OpenAlex 请求带 `sort=publication_date:desc`（否则结果集漂移）。
- 同步在后台线程执行（`std::thread::spawn`），通过 Tauri 事件 `sync://*` 回报进度，UI 不被阻塞。
