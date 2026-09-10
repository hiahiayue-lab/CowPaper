# CowPaper v0.2.2 独立 QA 包

状态：**Phase 1 QA 准备完成，候选验收尚未执行**

基线：`origin/main` / `33652b059fc20427da105976afd8ddabab423403`（`v0.2.1`）

本目录是 v0.2.2 的独立验收入口：

- [`QA_MATRIX.md`](./QA_MATRIX.md)：Search、Library、PDF、Visual、Motion 的逐项矩阵与证据要求。
- [`FINAL_REPORT_TEMPLATE.md`](./FINAL_REPORT_TEMPLATE.md)：最终候选报告模板，包含本地验证、CI 同 SHA、精确 macOS candidate、DB19 与用户数据检查。

## 边界

- 只验收 v0.2.2 候选；不得修改 v0.2.1、v0.2.0、v0.1.4。
- 不执行 migration，不创建 tag/Release，不向生产数据库导入 fixture，不触碰用户数据。
- Search 使用真实 runtime 优先；`docs/qa/library-search-v021/fixtures-rc3.json` 和现有纯状态测试只用于准备性契约/回归，不足以替代候选 runtime 证据。
- DB19 检查是完整性与不变式核验，不是迁移任务；不得为 QA 改 schema 或重建用户数据库。
- 视觉验收必须记录候选 SHA、macOS、屏幕缩放/DPR、窗口尺寸和截图路径；不能仅凭 DOM 或 fixture 截图判定通过。

## 开始条件

最终整合后，release owner 提供：

1. exact candidate SHA；
2. 对应的本地 disposable fixture DB / runtime 启动方式；
3. macOS `.app` 候选路径（同一 SHA）；
4. CI run URL 或 artifact，且 checkout SHA 与 candidate SHA 一致。

在这些条件满足前，矩阵状态只能是 `READY FOR INDEPENDENT QA`，不能写成 PASS 或 release sign-off。
