# CowPaper 阶段一：论文获取验证报告

- 生成时间：2025-08-21（UTC）
- 验证窗口：近 30 天 + 近 365 天（双窗口）
- 数据源：Crossref REST API、OpenAlex API、RSS/Atom 候选探测
- 原始数据：`phase1/report.json`
- 验证工具：`phase1/validate.mjs`（`node phase1/validate.mjs` 可复现）

## 一、总结论

**8 本目标期刊全部能通过 Crossref + OpenAlex 稳定发现论文，无需 RSS、无需任何出版社适配器即可完成阶段二~四的最小闭环。** RSS 只对 INFORMS 3 本可用，属于"更快发现"的可选通道，不影响闭环。

唯一薄弱点是 **Journal of Political Economy（JPE）**：Crossref 完全不提供摘要，OpenAlex 也只有约 59%（最新论文更低），约四成论文将长期处于"暂未取得摘要"状态。

## 二、汇总表

> 摘要覆盖率：Crossref 为近一年抽样 100 篇；OpenAlex 为近一年**全量**精确统计。百分比为时间点快照，可能随源站数据更新有小幅波动。

| 期刊 | 出版频率 | Crossref 30d | Crossref 1y | Crossref 摘要(1y) | OpenAlex 摘要(1y 全量) | RSS | 判定 |
|---|---|---|---|---|---|---|---|
| Management Science | 月刊 | 91 | 852 | 99% | 96% | ✅ 可用 | fullySupported |
| Marketing Science | 双月刊 | 6 | 96 | 85% | 77% | ✅ 可用 | fullySupported |
| American Economic Review | 月刊 | 12 | 141 | 88% | 87% | ❌ 404 | fullySupported |
| Journal of Marketing | 双月刊 | 5 | 94 | 95% | 95% | ⚠️ 超时 | fullySupported |
| Information Systems Research | 季刊 | 22 | 252 | 97% | 94% | ✅ 可用 | fullySupported |
| Journal of Political Economy | 月刊 | 14 | 146 | **0%** | **59%**（最新更低） | ❌ 403 | supportedWithMissingAbstracts |
| Journal of Management Information Systems | 季刊 | 0 | 44 | **0%** | **98%** | ❌ 403 | fullySupported（低频） |
| Econometrica | 双月刊 | 0 | 62 | 69% | 67% | ❌ 403 | supportedWithMissingAbstracts（低频） |

## 三、关键发现

### 1. Crossref 是最及时的发现源
30 天窗口内，Crossref 发现的论文数普遍高于 OpenAlex：

- Management Science：Crossref 91 vs OpenAlex 36
- ISR：22 vs 9
- JPE：14 vs 5

**结论**：OpenAlex 对最新论文的索引存在明显滞后。这印证了需求书 §6.1 的定位——**Crossref 做基础发现，OpenAlex 做第二套发现 + 摘要补充**。软件增量同步应以 Crossref 为主。

### 2. "Crossref 缺摘要 → OpenAlex 补" 是关键救命路径
两本期刊不向 Crossref 存摘要（0%），但 OpenAlex 完全或部分补齐：

- **JMIS**：Crossref 0% → OpenAlex 98%（46 篇中 45 篇）
- **JPE**：Crossref 0% → OpenAlex 59%

**结论**：需求书 §9 的摘要来源优先级（Crossref → OpenAlex）必须严格实现。缺失它，JPE、JMIS 将完全拿不到摘要。

### 3. JPE 是唯一真正的薄弱点
即使加上 OpenAlex，JPE 仍只有约 59% 摘要，且**最新论文摘要更缺**（最新 25 篇仅 44%）。意味着约四成 JPE 论文会处于 `waitingForAbstract`，不调用 DeepSeek（符合 §9）。

**结论**：
- 第一版可接受此现状（论文仍会展示，只是无 AI 分析）；
- JPE 是阶段五唯一值得考虑的"出版社摘要页适配器"候选（UChicago Press 公开摘要页，无需登录）。

### 4. RSS 只有 INFORMS 3 本确认可用
| 期刊 | RSS 探测结果 |
|---|---|
| Management Science / Marketing Science / ISR | ✅ 200 + 有效 Feed |
| AER | 404（URL 需另找，AEA 现无稳定的逐刊 RSS） |
| Journal of Marketing | 超时（SAGE 反爬/慢） |
| JPE / JMIS / Econometrica | 403（多为反爬，非"无 Feed"） |

**结论**：RSS 是可选的"更快发现"通道（§6.4），仅对 INFORMS 期刊生效。403 主要是反爬而非无 Feed，阶段五如需可针对具体出版社研究（不阻断）。

### 5. 低频刊的 30 天窗口可能为 0
JMIS（季刊）、Econometrica（双月刊）近 30 天均为 0 篇新文，但近一年分别有 44、62 篇。

**结论**：软件**不得**用"30 天是否有新文"判断期刊是否支持，应使用"上次成功同步时间"做增量（§7.2），并以"近一年存在论文记录"判断可发现性（§5.2）。

## 四、对后续阶段的落地建议

1. **发现顺序**（§6.1）：RSS（若配置且可用）→ Crossref（`from-index-date` 增量，主力）→ OpenAlex（补漏 + 补摘要）。
2. **摘要填充顺序**（§9）：RSS 摘要 → Crossref `abstract` → OpenAlex `abstract_inverted_index`（还原为文本）。空则 `waitingForAbstract`。
3. **补漏同步**（§7.3）：JPE/JMIS 这类"OpenAlex 摘要滞后"的期刊，7 天补漏应重点回填摘要。
4. **阶段五范围**：仅 JPE 需要评估"摘要页适配器"；其余 7 本无需任何适配器。
5. **OpenAlex 需带确定性排序**：`sort=publication_date:desc`，避免结果集漂移（本次验证中已证实不带排序会得到不同子集）。

## 五、覆盖结论（对照需求书 §17 阶段一清单）

| 验证项 | 结果 |
|---|---|
| Crossref 覆盖 | ✅ 8/8 |
| OpenAlex 覆盖 | ✅ 8/8 |
| RSS 是否存在 | ⚠️ 3/8 确认可用，其余反爬/404，不阻断 |
| 最近 30 天论文数量 | ✅ 6/8 有，2 本低频刊为 0（正常） |
| DOI 交集 | ✅ 各刊 Crossref/OpenAlex 样本高度重合 |
| 摘要覆盖率 | ⚠️ 6/8 ≥ 67%，JPE 59%、其余正常 |
| 是否需要出版社适配器 | 阶段一~四：不需要；阶段五：仅 JPE 待评估 |
