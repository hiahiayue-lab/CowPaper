# CowPaper v0.2.2 独立 QA Matrix

状态：**READY FOR INDEPENDENT QA / candidate gates not executed**

基线：`origin/main` / `33652b059fc20427da105976afd8ddabab423403`

范围：Search、Library hierarchy/membership、PDF attachment/reader、Discovery/Library/Settings visual consistency、high-frequency motion/reduced-motion，以及最终候选的 local/CI/macOS/DB19 integrity gate。

## 证据等级

| 标记 | 含义 | 可否单独作为候选通过证据 |
|---|---|---|
| `AUTO` | 纯函数、Rust 单测或静态 guard；不依赖 GUI | 只能覆盖该自动断言，不能替代 runtime/UI 行 |
| `FIXTURE` | disposable fixture/harness 的契约或渲染证据 | 只能证明 fixture/harness 场景，不代表真实 SQLite/Tauri/macOS |
| `RUNTIME` | 真实 Tauri runtime + disposable DB | 可以覆盖行为，但要记录 command/event/DB 证据 |
| `MACOS` | 精确 macOS `.app` candidate 上的人工或系统行为 | PDF reader、restart、picker、视觉/窗口行为必须用此等级 |
| `CI` | CI 在同一 SHA 的构建/测试结果 | 只证明该 CI job/target；不能替代本机 macOS candidate |

每行都要填写：`结果(PASS/FAIL/BLOCKED/N/A) · candidate SHA · 证据路径 · notes`。任何 `BLOCKED` 都必须写明缺失的 runtime、权限、artifact 或外部依赖。

## 0. Fixture 与隔离前置

现有 Search fixture：[`fixtures-rc3.json`](../library-search-v021/fixtures-rc3.json)，纯状态测试：[`librarySearch.rc3.test.ts`](../../../app/src/librarySearch.rc3.test.ts)。v0.2.2 可复用其 A–M 记录，但必须补足或确认以下 v0.2.2 场景：

- `AI` Collection（含 `AI / Governance` 子 Collection）、`ESG` Collection；`Core`、`Review`、`Zero Count` Tags；Paper `112` 为多字段命中锚点。
- Paper `101/102/105/112` 支持 `AI 治理`，Paper `103/104/105/112` 支持 `ESG 平台`，Paper `101/102/112` 支持 `人工智能`；精确混合查询 `人工智能 AI 治理 ESG 平台` 只返回 canonical Paper `112`。
- `AI` → `Collection` token → `ESG` Tag token → `governance` 的连续操作，验证 token 顺序/保留和结果交集。
- 一个 Paper 在多个字段命中时，结果只出现一行；`matched_fields` 唯一且只含可搜索字段，`snippets` 与其键集合一致、短且可安全渲染。
- 至少一个有 PDF、一个缺失路径、一个 System Default reader、一个有效 macOS `.app` reader、一个不存在/不可启动 reader；restart 前后保留 attachment/reader/排序状态。

Fixture 只能写入 disposable in-memory DB 或 disposable copy。禁止生产 seed、用户 DB、migration、Release/tag。

## 1. Search matrix

| ID | 场景与操作 | 通过标准 | 证据 | 等级 |
|---|---|---|---|---|
| S01 | Library 进入后检查 Search surface | 只有一个 Library toolbar Search；Discovery/Settings/sidebar/table/Inspector 无重复 Search | DOM count + toolbar screenshot | RUNTIME |
| S02 | 输入 `AI`，选择 Collection suggestion | 出现并保留 `AI` Collection token；未执行错误的全量/重复请求 | request trace + token snapshot | RUNTIME |
| S03 | 在已有 `AI` token 下选择 `ESG` Tag | `AI` Collection token 与 `ESG` Tag token 共存；token 选择消耗临时 suggestion text，不吞掉后续自由文本 | token screenshot + serialized query | RUNTIME |
| S04 | 接 S02–S03 输入并提交 `governance` | Collection/Tag/free text 按既定维度组合；结果交集与 fixture 期望一致；Enter 不添加错误 token/Action row | raw query + ordered paper IDs + suggestion DOM | RUNTIME |
| S05 | Field-specific Chinese title | 使用字段 suggestion/field token 将 `AI` 限定为中文标题搜索；不会匹配 English title-only 记录；token 显示准确中文字段意图 | field token/accessibility snapshot + result IDs | RUNTIME |
| S06 | Paper `112` 多字段命中 | 同一 Paper 只渲染一行；`matched_fields` 无重复、仅为允许字段；snippet 与命中字段对应且不泄漏 DOI/URL/publisher 等 display-only 字段 | raw response JSON + row screenshot | RUNTIME |
| S07 | 精确查询 `AI 治理`、`ESG 平台`、`人工智能` | 结果集合分别符合 fixture 契约；Latin 大小写不敏感；CJK 规范化对存储值/查询值对称 | raw responses + result IDs | RUNTIME |
| S08 | 精确混合查询 `人工智能 AI 治理 ESG 平台` | 只剩 Paper `112`；不因空格/CJK 分词丢失任一条件 | raw response + table screenshot | RUNTIME |
| S09 | Outside click | 关闭 suggestions，但保留 query、Collection/Tag tokens、已应用结果、selected Paper 与 Inspector；不触发重复 search | before/after state + request count | RUNTIME |
| S10 | Reopen / empty query | 重新聚焦恢复 query/tokens/results/selection；空 query 不倾倒全量 suggestions | two screenshots + state snapshot | RUNTIME |
| S11 | Escape 与 clear | Escape 只关闭开放 popover；明确 clear 才清空 query/results；tokens 不被意外清掉，或按产品定义逐个删除 | keyboard recording + state diff | RUNTIME |
| S12 | Real IME：`人工智能`、`AI 治理`、`ESG 平台` | composition 期间 Enter/Escape/ArrowUp/ArrowDown 不执行、关闭、导航；compositionend 后提交一次；必须是真实中文 IME，不接受粘贴替代 | screen recording + event/request trace | MACOS |
| S13 | Paper suggestion identity | 选择 Paper suggestion 只定位并选中既有 canonical row，query 不变成重复 title token；Inspector 跟随同一 Paper | selected paper ID + screenshot | RUNTIME |
| S14 | Multi-Collection / parent-child de-dup | Paper `109` 在 AI+ESG 中只有一行；parent/child identity 只有一个 canonical row；snippet/matched_fields 不重复计数 | raw rows + visible row count | RUNTIME |
| S15 | Display-only negative search | DOI、URL、publisher、volume、issue、pages 不在统一搜索命中，也不出现在 matched_fields/snippets | result JSON + negative assertions | RUNTIME |
| S16 | Workspace boundary | 带已提交 Library query 切到 Discovery/Settings 再回来：外部 workspace 不受过滤；回来 query/tokens/results/Inspector 保留 | workspace storyboard + invocation log | RUNTIME |

## 2. Library hierarchy / membership matrix

| ID | 场景与操作 | 通过标准 | 证据 | 等级 |
|---|---|---|---|---|
| L01 | Collection sibling reorder | 在同一 parent 下拖动/重排 Collection；parent_id 不变，排序在当前 runtime 生效 | before/after list + DB query | RUNTIME |
| L02 | Intentional nest | 明确将 root Collection 放入允许的 parent；只改变目标 Collection 的 parent，子项与 Paper membership 不丢失 | drag recording + `parent_id`/membership snapshot | RUNTIME |
| L03 | Intentional promote | 将 child Collection 提升到 root；只清空其 parent，兄弟顺序与 Paper membership 保持 | drag recording + DB snapshot | RUNTIME |
| L04 | Invalid hierarchy guard | 自身嵌套、循环或超过产品允许层级的 drop 被拒绝；无部分更新、无幽灵行 | failed-drop screenshot + DB transaction diff | RUNTIME |
| L05 | Collection reorder restart persistence | 退出并重启精确 candidate；同一 parent 下 Collection 顺序一致，且 query scope/selected Paper 不产生错误漂移 | before/restart/after screenshots + DB/app_state | MACOS |
| L06 | Tag reorder | 拖动 Tags 后列表顺序立即正确，zero-count Tag 仍在列表中 | screenshot + stored order value | RUNTIME |
| L07 | Tag reorder restart persistence | restart 后 Tag 顺序与退出前一致；不存在重复、丢失或按名称意外重排 | before/restart/after screenshots + app_state | MACOS |
| L08 | Zero-count Tag | `Zero Count` 可见、名称可读、计数为 0、视觉 dimmed；仍可点击/选中、仍为有效 drag target | DOM attrs + pointer/drag recording + screenshot | RUNTIME |
| L09 | Inspector Collection navigation | Inspector 中 Collection relation link 导航到对应 Collection scope；不改 Paper canonical identity，返回后 selected Paper 可恢复 | click path + scope/result snapshot | RUNTIME |
| L10 | Inspector Tag navigation | Inspector 中 Tag relation link 导航到对应 Tag scope；多 Tag AND/已有 Search token 组合不被静默替换 | click path + query/token snapshot | RUNTIME |
| L11 | Context menu navigation | Collection/Tag/Paper/Attachment context menu 的打开、关闭、命令对象准确；outside click/Escape 不改数据 | screenshot + command trace | RUNTIME |
| L12 | Library remove | 移出 Library 后该 Paper 从 Library rows/membership 消失，但 canonical `papers`、PDF attachment、history/recommendation snapshot 保留；再次同步/打开历史不复制数据 | before/after DB row counts + UI screenshots | RUNTIME |
| L13 | Selection/Inspector continuity | reorder、scope change、Search close/open 后 selection 只在产品定义需要时变化；Inspector 不出现空/错 Paper | state trace + Inspector screenshot | RUNTIME |

## 3. PDF / reader matrix

| ID | 场景与操作 | 通过标准 | 证据 | 等级 |
|---|---|---|---|---|
| P01 | Attach linked PDF | attachment 建立为 linked；原文件保留；Inspector child 行显示正确 provenance/status | file hash/path + DB row + screenshot | RUNTIME |
| P02 | Managed copy/move | copy 保留 source、destination 内容/hash 正确；move 仅在 destination 验证后删除 source；失败时 source 不丢 | file listing/hash + DB snapshot | AUTO/RUNTIME |
| P03 | Parent/child attachment selection | attachment 是 Paper 的 child；选中 child 时 Inspector/context actions 指向 attachment，Paper identity 不被替换 | table/child/Inspector screenshots + command trace | RUNTIME |
| P04 | Open from Inspector | 正常 attachment 从 Inspector 打开；按钮只在文件存在时可用，missing 状态提供 relink/recovery | macOS app observation + screenshot | MACOS |
| P05 | Open from context menu | Attachment context menu 的 Open/Reveal/Relink/Detach 映射准确；菜单打开不造成 table/Inspector layout shift | context screenshot + command trace | MACOS |
| P06 | System Default | Settings 选择 `System default`；Open PDF 交给 macOS 默认 PDF 应用；restart 后 preference 仍为 system | Settings screenshot + `open`/process observation + restart check | MACOS |
| P07 | macOS `.app` picker | 选择应用使用 macOS app picker；选定 `.app` 后路径持久化为绝对路径；cancel 不覆盖旧值；非绝对/空值被拒绝 | picker recording + Settings before/after | MACOS |
| P08 | Custom reader | 有效 `.app`/executable reader 能打开 PDF；reader preference 与 attachment path 分离保存 | process observation + DB setting | MACOS |
| P09 | Missing reader fallback | 已保存 reader 被移除/不可启动时，给出明确 fallback/recovery；不静默假成功，不损坏 attachment；System Default fallback 可重新选择 | missing-reader screenshot + status/error + DB invariants | MACOS |
| P10 | Reader restart | 退出/重启后 reader mode/path、attachment status、managed/linked path、missing state 都一致 | before/restart/after screenshots + settings query | MACOS |
| P11 | Library remove preserves PDF/history | Paper 移出 Library 后 canonical Paper、attachment rows/files、recommendation/history records 按产品契约保留；Library UI 不再显示该 membership | DB snapshot + UI/history screenshots | RUNTIME |
| P12 | Missing linked file | 文件被移走后显示 missing；Open/Reveal 不假成功；Relink 到有效 PDF 后状态恢复，canonical metadata/history 不被覆盖 | filesystem action + screenshots + DB diff | MACOS/RUNTIME |

## 4. Visual matrix

截图至少覆盖 `1440×982`、`1512×982`、`1536×982`、`1000×982`、`740×982`、`600×982`；记录 DPR/zoom。除非候选明确改变设计契约，否则沿用 [`DESIGN.md`](../../../DESIGN.md) 的 168px rail、54px workspace row、300px Inspector、紧凑 table rows 和语义色。

| ID | Surface/state | 通过标准 | 证据 | 等级 |
|---|---|---|---|---|
| V01 | Discovery typography | system UI 层级、标题/metadata/状态对比清楚；无 Library serif/搜索残留 | screenshots + computed style spot-check | MACOS |
| V02 | Library typography | title/Chinese title/metadata/年份/计数层级正确；年份/计数 tabular；Inspector reading text 才使用 serif | screenshots + computed style | MACOS |
| V03 | Settings typography | labels/helper/error/success 使用统一 UI scale；长文本换行不挤压控件 | screenshots at wide/narrow | MACOS |
| V04 | Shared toolbar/chrome | Discovery/Library/Settings workspace row 高度、边界、按钮基线一致；OS title-bar space 未被伪造 | cross-workspace storyboard | MACOS |
| V05 | Sidebar | 168px rail（600px narrow contract 除外）、层级缩进、counts、active/hover/focus 一致；Tag 零计数仍可读 | screenshots + measured anchors | MACOS |
| V06 | Search control | 一行 28px control；icon/tokens/input/clear 对齐；focus ring 可见；无重复控件或溢出 | focused/unfocused screenshots | MACOS |
| V07 | Search popover | trigger 下方 anchored、无 table/Inspector 位移、静态打开/关闭、分组层级与空态清楚 | before/after screenshots + anchor measure | MACOS |
| V08 | Library table | header/rows 共享 column grid；separator/selection/hover 不改变 row height；长字段 ellipsis，无 page/list overflow | screenshots + overflow metrics | MACOS |
| V09 | Inspector | continuous light surface；Metadata only；serif title/abstract；Collection/Tag/PDF groups、child attachment、actions 对齐 | selected Paper screenshots | MACOS |
| V10 | Settings controls | PDF root/picker/reader/template/warning/saved/error 状态局部归属明确，控件 wrapping/focus/disabled 正确 | state screenshots | MACOS |
| V11 | Popovers/context menus | status、column、relation、Collection/Tag/Paper/Attachment menus trigger-owned、可关闭、无错误层级/裁切 | open-state screenshots | MACOS |
| V12 | Empty/loading/error/success | Discovery/Library/Settings/Inspector/PDF empty/error 状态保留上下文，文本说明状态并给出一个可执行 recovery | state sheet screenshots | MACOS |
| V13 | Responsive/overflow | 六种 viewport 下无 document/app-shell horizontal overflow；窄宽按设计隐藏 Note/Authors/Inspector，不出现第二 Search toolbar | browser metrics + screenshots | MACOS |
| V14 | Contrast/accessibility | 状态不只靠颜色；disabled/dimmed 与可读性区分；所有 keyboard controls 有可见 focus target | focus/state sheet | MACOS |

## 5. Motion / reduced-motion matrix

默认契约：typing、Search filtering、suggestion open/close、table result replacement、row selection、sidebar filtering/reorder feedback、Inspector refresh 均无动画。任何偶发 panel motion 只能是明确的 transform/opacity，且 reduced-motion 下去除移动。不得出现 `transition: all`、layout-property animation 或 width progress interpolation。

| ID | 高频路径 | 通过标准 | 证据 | 等级 |
|---|---|---|---|---|
| M01 | Search typing | 每个字符更新没有 debounce-induced stale UI、fade、height jump 或 keyframe restart；输入焦点稳定 | rapid typing recording + computed style | MACOS |
| M02 | Search filtering/results | result replacement instant；popover 不推动 table/Inspector；连续 query 不显示旧结果闪回 | recording + request/result trace | MACOS/RUNTIME |
| M03 | Table/sidebar selection | row selection、sidebar scope、Collection/Tag token 变化无 movement；selection state 可由颜色+文本/outline 理解 | recording | MACOS |
| M04 | Reorder/drop feedback | drag feedback 只标示目标/位置，不触发布局动画；完成后顺序立即落定 | drag recording + DOM class timing | MACOS |
| M05 | Inspector/PDF status | Inspector refresh、attachment selection、missing/error/success 不用遮挡性动画传递状态；progress 不动画 width | recording + CSS inspection | MACOS |
| M06 | Reduced motion | OS `prefers-reduced-motion: reduce` 下 movement 被移除，但 state/progress/text/opacity information 保留；键盘/IME 语义不变 | matched normal/reduced recordings | MACOS |
| M07 | Static source guard | 全局 CSS 无 `transition: all`；Search/high-frequency selector 无 transition/keyframes；progress 使用 instant 或 transform-only | `rg`/computed-style output | AUTO |

## 6. Final candidate gates

| ID | Gate | 通过标准 | 证据 | 等级 |
|---|---|---|---|---|
| G01 | Local validation | exact candidate SHA 上 frontend typecheck/build、Search pure tests、fixture contract、Rust check/test、`git diff --check` 全部通过 | command transcript + SHA | AUTO |
| G02 | CI same SHA | required CI job checkout SHA 与 candidate 完全一致；frontend/Rust tests 和目标 artifact 成功 | CI URL + artifact metadata | CI |
| G03 | Exact macOS candidate | `.app` 来源 SHA 可追溯；签名/架构/启动、restart、picker、System Default、visual/motion checks 均针对该 bundle | app path + hash + screenshots | MACOS |
| G04 | DB19 integrity | `PRAGMA user_version = 19`；v19 tables/FTS/index 存在且结构符合现有代码；Search document 与 Library canonical rows 一致；无意外 schema drift | read-only SQLite snapshot | RUNTIME |
| G05 | User-data preservation | candidate validation 前后 user DB backup/hash/row invariants 可比；canonical papers、PDF attachments/files、recommendation/history、settings、Collection/Tag membership/order 无非目标变化 | before/after manifest + SQL checks | RUNTIME/MACOS |
| G06 | Migration boundary | QA 没有执行/提交 migration；若 candidate 自带 v19 migration，只验证既有路径/完整性，不改 migration 或升级版本 | command log + git diff | AUTO |
| G07 | Release boundary | 未创建 tag/Release；未改 v0.2.1/v0.2.0/v0.1.4 文件；报告记录检查结果 | git refs/status + diff path list | AUTO |

## 建议的本地验证顺序

在 exact candidate checkout、且不修改用户 DB 的前提下执行：

```text
cd app
npx tsc --noEmit
npx vite build
node --experimental-strip-types src/librarySearch.test.ts
node --experimental-strip-types src/librarySearch.rc2.test.ts
node --experimental-strip-types src/librarySearch.rc3.test.ts
node ../docs/qa/library-search-v021/rc3-fixture.test.mjs

cd src-tauri
cargo check
cargo test
```

之后再做真实 Tauri/macOS runtime 与视觉证据。不要把 `cargo test`、fixture test 或 headless screenshot 单独当作完整候选验收。

## Exit criteria

候选只有在以下条件同时满足时才可报告 `PASS / release candidate accepted`：

1. 所有 `RUNTIME`/`MACOS` 行均有可复核证据；
2. G01–G07 全部通过；
3. FAIL 已修复并在同一 candidate SHA 的新 candidate 上重跑，或由 release owner 明确接受并链接 finding；
4. 没有未解释的用户数据、canonical identity、PDF attachment、history/recommendation 或 DB19 不变式差异。

本文件本身不宣称基线或任何候选已通过。
