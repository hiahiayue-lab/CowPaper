# CowPaper —— macOS 期刊论文订阅与推荐

本地运行的期刊论文订阅 + AI 推荐工作台。订阅期刊 → 自动发现新论文（Crossref/OpenAlex）→ DeepSeek 生成中文摘要并按标签评分排序。

## 目录结构

```
CowPaper/
├── phase1/            # 阶段一：论文获取验证工具（node validate.mjs）+ 报告
├── docs/              # 决策记录、阶段一覆盖报告
├── app/               # Tauri 应用（前端 + src-tauri）
│   ├── src/           # 前端（原生 TS + HTML/CSS，无 React）
│   └── src-tauri/     # Rust 内核（SQLite、同步、AI 队列、命令）
├── .rustup/ .cargo/   # 工作区内 Rust 工具链（已加入 .gitignore）
└── dev-env.sh         # 开发环境变量（构建前 source）
```

## 构建 / 运行

前置：macOS + Xcode 命令行工具（Rust 已装于工作区内，无需全局安装）。

```bash
cd CowPaper
source ./dev-env.sh
cd app
npm install          # 首次
npm run tauri dev    # 开发模式（热重载）
npm run tauri build  # 打包 .app（产物在 app/src-tauri/target/release/bundle/macos/CowPaper.app）
```

## 当前进度

- [x] 阶段一：8 本目标期刊覆盖验证（`docs/phase1-report.md`）
- [x] 阶段二：本地订阅闭环（期刊 / 同步 / 去重 / SQLite / 期刊状态）
- [x] 阶段三：DeepSeek AI（中文标题/摘要/一句话总结 + 标签评分 + 推荐视图）
- [x] 阶段四：日常体验（启动自动同步 / 每日计划 / 菜单栏托盘 / 通知 / 收藏·已读·忽略）
- [x] 阶段四增补：**AI Analysis Queue**（持久化、并发 2、暂停/继续/停止、retry/429、实时进度、退出恢复）
- [x] Round 3.5 加固：唯一 SyncCoordinator（禁止同步重入）、DeepSeek 错误三级分类（Retryable/GlobalConfig/Paper + 配置错误全局暂停）、duplicate tag 规范化（canonical 集合 + 最高分）、API Key 迁移 Keychain、AI last-run 摘要、migration 版本化（PRAGMA user_version + 事务）
- [ ] 阶段五：缺口适配（仅 JPE 待评估）

## 关键实现说明

- **AI 队列**（`src-tauri/src/ai_queue.rs`）：全局唯一协调器，并发 2；论文状态 `pendingAnalysis/queued/analyzing/analysisSucceeded/analysisFailed/waitingForAbstract`；队列状态 `idle/running/pausing/paused/stopping`；状态持久化到 SQLite `app_state`；暂停=不再领新任务且让当前请求完成；停止=已完成保留、未完成回退 `pendingAnalysis`；429/5xx/网络最多 3 次重试（Retry-After 或指数退避），配置错误（401/400/403）暂停整队；进度经 `ai://progress` 事件推送。
- **同步与 AI 解耦**（§三十四）：同步完成即报告，新论文 id 经 `sync://done.newPaperIds` 交前端，按「同步后自动分析新论文」设置入队，历史积压只提示不静默全跑。
- **自动同步**：启动时 `maybe_auto_sync`（距上次同步 >30 分钟才执行）；Rust 每日调度器（默认 09:00，每天一次，`lastDailySyncDate` 防重复，退出后下次启动 catch-up）；手动/托盘即时。
- 去重优先级（需求书 §8.2）：DOI → 出版社文章 ID → OpenAlex Work ID → 标题+ISSN+年份。
- 摘要来源：Crossref `abstract` → OpenAlex `abstract_inverted_index`；缺失则 `waitingForAbstract`。
- DeepSeek API Key 存 **macOS Keychain**（经 `SecureStore` 抽象，Rust 读取，前端不保存真实 Key；旧 localStorage Key 启动时一次性迁移）。
- 前端为 **Vite + 原生 TypeScript**（项目初始脚手架即如此，未引入 React）。

## 测试

```bash
cd app/src-tauri && cargo test   # 9 项：去重/迁移/队列 DB 机制/retry/协调器集成（20 篇批处理、暂停-继续、停止、单篇失败、429 重试）
```
