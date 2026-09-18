# CowPaper 0.4.1

CowPaper 0.4.1 improves the day-to-day workflow for finding and organizing literature.

## 文献库搜索与文集

- 改进搜索建议的字段显示与文集名称检索。
- 文集浏览与搜索筛选状态保持清晰分离。
- 选择文集搜索建议后，可明确使用文集筛选结果。

## PDF 文件名与预览

- PDF 文件名会根据当前命名模板与有效文献元数据自动同步。
- 缺失字段使用安全的 `none` 占位，全部缺失时使用 `none.pdf`。
- 改进文献库设置中的命名预览，并保留旧模板的 DOI 兼容性。

## 发布与自动更新

- 修复 macOS 与 Windows 发布资产不完整的问题。
- 自动更新元数据现在由单一发布流程生成，并同时包含两个平台的签名资产。
