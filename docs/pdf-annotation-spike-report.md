# CowPaper PDF Annotation Extraction Spike

日期：2026-09-03  
分支：`spike/pdf-annotations`  
基线：`origin/main` @ `6ad3e5a1cf4becc850f15d300a053abbc5edfccb`（`feat: add linked pdf library attachments`）  
范围：只研究从 linked PDF 读取已有 annotations；不实现内置 PDF Reader，不写回用户 PDF，不新增正式 migration。

## Executive conclusion

PDF annotations extraction 是可行的，但 highlight 的 `quoted_text` 只能判定为 **PARTIAL**：对有文本层、合法 `/QuadPoints`、可正确还原文字几何的 PDF，恢复质量可以很好；对扫描件、缺少 ToUnicode 的字体、复杂阅读顺序或损坏 annotation，不能承诺可靠恢复。`/Contents` 是批注/comment，不是高亮原文，不能当作 quote 的替代品。

推荐路线是 `pdfium-render` 作为主 PDF 引擎：它暴露了 annotation collection、`/NM` 等通用字段、text markup 的 attachment points（quadpoints）以及按 annotation 获取页面文字/字符的 API。它不是纯 Rust，但 PDFium 是 Chromium 使用的成熟引擎，并且官方 PDFium 代码采用 Apache 2.0 许可。部署时需要为 macOS 和 Windows 分发并验证对应的 PDFium runtime。必要时可用 `lopdf` 对同一份 bytes 做窄范围 raw dictionary pass，补足 PDFium 高层 API 未暴露的 `/Popup`、`/IRT` 或自定义字段；不建议单独用 `lopdf` 做文字几何恢复。

实际产品应先做 read-only import + Inspector + guarded Library Search，优先支持嵌入 PDF 的 Highlight、Underline、StrikeOut、Text、FreeText。不要把 Skim 的默认 sidecar/xattr 或 Zotero 的数据库 annotation 当成 PDF `/Annots` 处理；它们应是后续独立 source adapter。建议在 `v0.3.0` 作为受控功能进入实现，`v0.2.x` 仅适合隐藏的 research prototype。

## PDF annotation standard support

标准入口是每个 page dictionary 的 `/Annots` 数组；数组元素通常是 annotation dictionary 的 indirect reference。Adobe PDF Reference 对 common annotation dictionary、markup annotations、text annotations、free-text annotations 和 text markup 的定义见 [PDF Reference 1.7](https://opensource.adobe.com/dc-acrobat-sdk-docs/pdfstandards/pdfreference1.7old.pdf)；下列规则与该规范及 [PDFium 的 annotation API 注释](https://pdfium.googlesource.com/pdfium/+/main/public/fpdf_annot.h)一致。

| 字段 | 研究结论 | CowPaper 处理 |
|---|---|---|
| `/Annots` | page-level annotations 的容器，不等于 page content；需要跟随 indirect references，并容忍缺失或 malformed entries。 | 每页扫描一次，只收目标 subtype；保留原始 object/reference 信息用于诊断，不把 object number 当身份。 |
| `/Subtype` | 目标类型为 `/Highlight`、`/Underline`、`/StrikeOut`、`/Text`、`/FreeText`。另有 Link、Ink、Stamp、Widget 等，不应误判成用户 quote。 | 映射为 `highlight`、`underline`、`strikeout`、`text`、`freetext`；其他类型进入 unsupported 状态/诊断。 |
| `/Rect` | 必填的 annotation rectangle，定义 page 上的外接框；对 text markup 可能只是整个多行选择的外接框。 | 仅作导航和无 quadpoints 时的低置信度 fallback；不能用整个 Rect 直接选字。 |
| `/QuadPoints` | text markup 的 8 × n 个坐标；每 8 个数字对应一个四边形，顺序是 upper-left、upper-right、lower-left、lower-right 的 Z pattern。每个 quad 覆盖一个 word 或 contiguous word group。 | 按 8 个数字切片；逐 quad 选 glyph；原样存储几何。若长度不是 8 的倍数，判为 malformed。 |
| `/Contents` | 对 Highlight、Underline、StrikeOut 等 markup，通常是 pop-up/comment 中显示的文字；对 FreeText 是直接显示的文字；对 Text 是 sticky note 内容。 | 进入 `comment` 或 `freetext` 内容，绝不当作 `quoted_text`。 |
| `/NM` | 可选 annotation name；规范只保证同一 page 内唯一，不是全 PDF 全局 UUID。 | 作为首选 external id，但 key 必须带 `attachment_id + page_index`；缺失、重复或跨 app 不稳定时回退 fingerprint。 |
| `/T` | markup 的 title-bar text，约定上表示作者/添加者。 | 映射 `author`；空值正常。 |
| `/M` | 最近修改时间；日期格式可能是 PDF date string，也可能是 viewer 可接受的其他 string。 | 映射 `modified_at`；不参与 identity。 |
| `/C` | 颜色数组；对 markup 通常是显示颜色，具体 opacity/appearance 仍可能由 `/CA` 或 `/AP` 影响。 | 读取为标准化颜色；无法解析时保留 raw metadata。 |
| `/Popup` | 通常是与 markup 关联的 pop-up annotation 的 reference；popup 自身可能没有独立业务内容。 | 跟随 parent/child 关系读取，但只创建一个业务 annotation row，避免把 Popup 重复导入。 |
| `/IRT` | “in reply to” reference；规范要求 reply 与原 annotation 在同一页，表示 threaded comment/state。 | 解析成 reply parent external id；v0.3.0 可先保留 raw metadata，UI 不把 reply 当作独立 quote。 |
| `/Text` | sticky note，附着在 point/Rect，关闭时显示 icon，打开时显示 note text；`/Name` 是 icon 名，和 `/NM` 不同。 | `kind=text`，`quoted_text=NULL`，`comment=/Contents`。 |
| `/FreeText` | 文字直接画在 page 上；通常使用 `/Contents`、`/DA`、`/RC`、`/DS`，没有 text markup 的 QuadPoints 语义。 | `kind=freetext`；其 visible text 作为 annotation text/comment，不伪造 quote。 |

标准还明确指出，annotation 若存在 `/AP` appearance stream，实际渲染可能优先使用 appearance，而不是重新解释 `/QuadPoints`。因此 prototype/产品应读取逻辑字段，同时不要用渲染结果反推 annotation identity。

## Highlight text extraction

结论：**FEASIBLE / PARTIAL / NOT RELIABLE = PARTIAL**。受控文本型 PDF 可以恢复；对任意用户 PDF 不能承诺 100% 可靠。

### Recommended extraction pipeline

1. 只读打开 PDF，并在 scan 开始时记录 `content_sha256`、文件大小、页数和打开错误。永远不对输入做 save/flatten/rewrite。
2. 枚举每一页的 `/Annots`，只选择目标 subtypes；读取 `/Rect`、全部 `/QuadPoints`、`/Contents`、`/NM`、`/T`、`/M`、`/C`、`/Popup`、`/IRT`。对 indirect object、缺失 key、坏 array 做容错。
3. 同时提取 page text layout：每个 Unicode character/glyph 至少需要 text、bbox/quad、baseline/angle、page-space coordinates 和 source text object。优先使用引擎的 ToUnicode 映射；没有可靠 Unicode mapping 时不要猜字符。
4. 将每个 annotation quad 变换到与 text glyph 相同的 PDF default user space。处理 `/Rotate`、CropBox/MediaBox、不同 library 的 y-axis convention，并保留原始 geometry。不要把屏幕像素坐标直接与 PDF 坐标比较。
5. 对每个 quad 单独做 glyph selection。优先使用 glyph quadrilateral 与 annotation quad 的 intersection/center-in-quad，并允许很小的浮点 tolerance；不要用整段 annotation Rect 选字，否则多行、邻栏和旁边文字会被带入。
6. 基于几何恢复 reading order：先按 writing angle/line baseline 聚类成行，再按行的阅读方向排序，行内按 baseline direction 排序；最后根据相邻 glyph gap 插入空格。PDF content stream 中的 object order 只能作为 tie-breaker，因为 PDFium 自己也注明定义顺序不一定等于用户视觉阅读顺序，见 [`PdfPageText::all()`](https://docs.rs/pdfium-render/latest/pdfium_render/prelude/struct.PdfPageText.html)。
7. 多个 QuadPoints 按 page geometry 排序，而不是盲信 array 顺序。每个 quad 保持为 fragment；只有在同一行/同一段、间距合理时才合并。跨行连接使用换行或规范化空格，必须同时保留 raw quads 以便回看和诊断。
8. 输出 `quoted_text`、`comment`、`extraction_status` 和可选 confidence。没有可验证文字时输出 `quoted_text=NULL`，而不是把 `/Contents`、OCR 猜测或邻近文字伪装成 quote。

### Known layout problems

| 情况 | 风险 | 建议 |
|---|---|---|
| reading order | PDF text objects 常按绘制顺序写入，可能先右栏后左栏，或 header/body 顺序异常。 | 以 glyph geometry 为主；对多栏做 line/column clustering；复杂页面保留 low-confidence。 |
| ligatures | `/ToUnicode` 可能返回 `ﬁ`，也可能展开成 `fi`；无 ToUnicode 时 glyph 可能没有可逆 Unicode。 | 展示文本保留 extractor 的可证实结果；fingerprint 使用 NFKC 规范化，不能为了“好看”凭空展开。 |
| hyphenation | 行尾 `-` 可能是印刷连字符，也可能是排版断词；盲删会改原文。 | 仅在同一词跨行、下一行延续且规则可信时做 display normalization；保存 raw/exact quote 或标记 heuristic。 |
| multi-column | 以整段 Rect 选字会把另一栏纳入；一个 annotation 也可能跨多个 column。 | 每个 QuadPoints 独立匹配；跨栏选择以 geometry order 输出并标记 ambiguous，不承诺语义段落。 |
| rotated pages/text | `/Rotate`、旋转文字和 renderer viewport transform 会使 x/y 直排序错误。 | 以 quad 的第一条边估计 writing angle，在局部坐标系排序，再映射回 page space。 |
| scanned PDF | 页面有 image 但没有 text glyph；quadpoints 仍然可能存在。 | 显示“Scanned PDF / no text layer”；v0.3.0 不隐式 OCR。未来 OCR 必须另存结果和 confidence，不能覆盖原 quote。 |
| OCR absent | 没有 OCR 时不存在可验证 quote；Contents 也可能只是用户 comment。 | 只显示 page、annotation type、comment、位置和失败原因。 |
| multiple fragments | 一次选择可能对应多个 quad，且 array 顺序未必是用户阅读顺序。 | 保留 `fragments[]` 的原始几何，几何排序后拼接；无法消歧就展示多个 fragments。 |

Prototype 已验证最小可行路径：`Contents="Reviewer comment is not the quoted text."` 的 Highlight 被恢复为页面真实文字 `Reliable quoted text comes from page geometry, not Contents.`；一个包含两个 QuadPoints 的 Highlight 被恢复为 `Multiple highlight fragments can be joined in reading order.`。

## PREVIEW COMPATIBILITY

macOS Preview 的官方用户指南列出 Highlight Selection、Note、Text 等 markup 工具，并说明使用 File > Save、Export 或 Export to PDF 保存后可继续编辑；使用 Print > Save as PDF 则会 flatten。Apple 的 [PDFKit annotation API](https://developer.apple.com/documentation/pdfkit/pdfannotation) 和 [`quadrilateralPoints`](https://developer.apple.com/documentation/pdfkit/pdfannotation/quadrilateralpoints) 也公开了 text markup、contents、popup、markup type 和 page-space quadpoints。

- Highlight 保存位置：保存的 editable PDF 内，page `/Annots` 中的 text markup annotation；通常依赖 `/QuadPoints`，不是 quote string。
- Underline 保存位置：同上，`/Subtype /Underline` + `/QuadPoints`。
- Comment/Note 保存位置：sticky note 对应 `/Subtype /Text`、`/Rect`、`/Contents`，可能关联 `/Popup`；直接可见文字更接近 `/FreeText`。
- 是否写回 PDF：是，当前官方文档明确区分 editable save/export 与 flattened print；本 spike 没有改动任何用户 PDF。
- sidecar/database：没有找到当前 Preview editable annotation 的官方 sidecar/database 语义；产品应以 PDF `/Annots` 为 source of truth，不依赖 Preview app state 或 autosave history。
- stable annotation ID：PDFKit 的 `PDFAnnotationKey.name` 语义是“同一 page 内唯一”，对应 PDF `/NM` 语义，但 key 是 optional/page-scoped；不能当 CowPaper 全局稳定 UUID。

结论：Preview 的标准嵌入 PDF 是 v0.3.0 的主要兼容目标；必须接受用户通过 Print > Save as PDF 生成的 flattened copy，此时原 annotation metadata 已不可恢复。

## ADOBE COMPATIBILITY

Acrobat 的官方文档说明可对选中文字添加 Highlight、Underline、Strikethrough 和 comment，也支持在 Comments pane 中管理。官方文档还说明 comments 可保存在 PDF，并可显式导出为 FDF/XFDF；见 [Add and manage comments](https://helpx.adobe.com/acrobat/web/share-review-and-export/review-pdfs/add-manage-comments.html) 和 [Importing and exporting comments](https://helpx.adobe.com/acrobat/using/importing-exporting-comments.html)。

- Highlight 保存位置：标准 PDF page `/Annots`，`/Subtype /Highlight`、`/QuadPoints`、`/C`，可有 `/Contents`/`/Popup`。
- Underline 保存位置：`/Subtype /Underline` + `QuadPoints`。
- Comment/Note 保存位置：可作为 markup 的 `/Contents`/`Popup`，或独立 `/Text` sticky note；reply 使用 `/IRT`。
- 是否写回 PDF：正常本地 Save 会把 annotations 写入 PDF；FDF/XFDF 是用户明确选择导出时的 sidecar。Acrobat web/managed review 还可能有 Document Cloud/server-side review state，不能假定所有 comment 都在本地 PDF。
- sidecar/database：FDF/XFDF 是标准的可选 sidecar/export；managed review 可能由服务端保存。导入 CowPaper 时只能读取本地 PDF，除非未来另做 XFDF connector。
- stable annotation ID：Acrobat 通常会有 annotation name，对应 `/NM`；但规范只要求 page-scoped optional identity，复制/导出/重写时不能无条件视为永不变化。

结论：Acrobat 是最完整的标准 PDF source；也最容易暴露 `/IRT` threaded replies、rich contents 和 appearance-vs-logical-field 差异。v0.3.0 先导入主 annotation，保留 reply/raw metadata。

## SKIM COMPATIBILITY

Skim 的官方文档明确说明：默认情况下 Notes 和 highlights 不写入 PDF data，而是保存为 extended attributes；Skim 也支持 `.skim` notes 文件，并可用 File > Export… > PDF > With Embedded Notes 输出一份含标准 PDF annotations 的 copy。其公开的 [Skim Notes keys](https://sourceforge.net/p/skim-app/wiki/Skim_Notes_Keys/) 包含 `pageIndex`、`contents`、`quadrilateralPoints`、`color`、`modificationDate`、`userName` 等，但没有 documented portable annotation UUID key。

- Highlight 保存位置：默认 SkimNotes archive/plist，通常 extended attribute 或 `.skim`/PDF Bundle sidecar；只有 With Embedded Notes export 才进入输出 PDF 的 `/Annots`。
- Underline 保存位置：同上，SkimNotes 的 `quadrilateralPoints`；embedded export 才可按标准 `/Underline` 读取。
- Comment/Note 保存位置：Skim 自定义 Note/Anchored Note 的 `contents`，可能还有 rich text/image；默认不在 PDF data。Skim 的 custom anchored note 不应假设能完全映射为标准 `/Text`。
- 是否写回 PDF：默认否；明确 embedded export 会生成新的 PDF copy。普通 note/highlight 与 embedded copy 不是同一个物理文件语义。
- sidecar/database：extended attributes、`.skim` notes 文件、PDF Bundle 都可能是 source；它们可能因云盘、文件复制、跨平台传输丢失。
- stable annotation ID：公开 keys 没有稳定 external ID；数组位置、xattr 顺序、object number 都不可靠。必须使用 geometry/text/comment fallback fingerprint。

结论：不要把“打开 Skim 但没有 embedded export”的 PDF 当作没有 annotations。v0.3.0 可明确支持 embedded copy；Skim native sidecar 需要另一个 macOS-specific adapter，暂不纳入 PDF `/Annots` import。

## ZOTERO COMPATIBILITY

Zotero 官方文档明确说明，Zotero 自己创建的 PDF annotations 存在 Zotero database，而不是 PDF file；这样可以做 conflict-free sync、tagging 和 filtering。外部嵌入 annotations 在 Zotero reader 中可见但默认 read-only，Import Annotations 后会转入 Zotero database；Export PDF 才会生成带 embedded annotations 的 PDF。见 [Why does Zotero store PDF annotations in its database](https://www.zotero.org/support/kb/annotations_in_database) 和 [Zotero PDF Reader](https://www.zotero.org/support/pdf_reader)。

Zotero 当前公开 schema 的 `itemAnnotations` 也说明了这种模型：`parentItemID` 关联 attachment，`type`、`authorName`、`text`、`comment`、`color`、`pageLabel`、`sortIndex`、`position`、`isExternal` 均在数据库中，见 [`userdata.sql`](https://github.com/zotero/zotero/blob/main/resource/schema/userdata.sql)。

- Highlight 保存位置：Zotero-native annotation 在 Zotero SQLite `itemAnnotations.text + position`；Export PDF 后才有 PDF `/Annots` copy。
- Underline 保存位置：同上。
- Comment/Note 保存位置：`comment`；note/text 还有 annotation type 和 position；不应从 linked PDF 的 `/Contents` 假定能读取 Zotero-native comment。
- 是否写回 PDF：native reader 默认不写回；Export PDF / Save As 生成嵌入 copy。外部 reader 写回的 standard annotations 可在 Zotero 中以 external/read-only 方式显示或导入。
- sidecar/database：核心 source 是 Zotero SQLite database，不是 PDF sidecar；linked attachment path 由 `itemAttachments` 管理。
- stable annotation ID：Zotero annotation item key 在 Zotero library 内可用，且 position JSON 可含 pageIndex/rects；但它不是 PDF `/NM`，离开 Zotero database 不可用于 CowPaper 的 PDF import。

结论：CowPaper 的 PDF adapter 只处理 Zotero 导出的 embedded PDF；如果未来要读取 Zotero-native annotations，必须读取用户授权的 Zotero database/connector，并按 attachment identity 关联，不能把 linked PDF 当唯一 source。

## Rust library comparison

| library | annotation metadata | text/geometry | macOS/Windows | license/operational risk | 结论 |
|---|---|---|---|---|---|
| `pdfium-render` | 有 page annotation collection，支持 common fields、annotation type、attachment points/quadpoints；当前 API 还提供 annotation-aware text/char extraction。 | 强；PDFium text chars、segments、page-space geometry，并有 `for_annotation` / `chars_for_annotation`。 | **YES**，但需要随 app 打包并加载对应 PDFium dylib/dll；需验证 arm64/x86_64 与 Windows runtime。 | wrapper 为 permissive Rust crate；PDFium upstream 为 Apache 2.0。运行时二进制供应链和版本 pinning 是主要成本。 | **推荐主引擎**。 |
| `lopdf` | 很强的 raw object/dictionary 访问，`get_page_annotations()` 直接给 `/Subtype`、`/Rect` 等；便于保留未知字段、`/Popup`、`/IRT`。 | 弱到中；有 text extraction/chunks，但不是完整 glyph geometry/reading-order engine。 | **YES**，纯 Rust，构建简单。 | permissive；面对 untrusted PDF 仍需 decompression limits/strict error policy。 | 适合 metadata fallback，不适合单独恢复 quote。 |
| `mupdf` / mupdf-rs | annotation type、quadpoints、PDF page API 很完整，底层引擎成熟。 | 强；structured text、text words/chars、rendering 都成熟。 | **YES**，但需要 C/C++ build/runtime 处理。 | `mupdf` crate 与 MuPDF 为 AGPL-3.0；商业发行需 Artifex commercial license，见 [MuPDF license](https://mupdf.readthedocs.io/en/1.27.0/license.html)。 | 技术上强，商业许可使其不是默认选择。 |
| 其他 pure-Rust PDF/text crates | 不同 crate 对 annotations、glyph geometry、rotation、malformed PDF 覆盖不一致。 | 尚不足以作为 CowPaper 的跨平台基线；需逐文件集 benchmark。 | 通常构建简单。 | 供应链成熟度、兼容矩阵和回归资源不足。 | 暂不选；纯 Rust 不是自动优势。 |

推荐实现分层：`pdfium-render` 负责打开、页面文字 geometry、render QA 和 text-markup quadpoints；对 `/Popup`、`/IRT`、`/RC` 等 PDFium 高层未直接暴露的 raw dictionary 字段，再按需要用 `lopdf` 对相同 bytes 做只读 metadata pass。只有在实际 license 评审接受 AGPL/commercial license 后，才重新评估 MuPDF。

## Library Search integration

未来 Annotation Search 应索引三个独立字段：

- `quoted_text`：原始/规范化后可证实的 PDF text layer quote；不可恢复时为 NULL，不把失败文本写入 FTS。
- `comment`：用户写入的 note/comment，包括 markup 的 `/Contents`，但不包含 highlight quote。
- `translation`：AI 生成的中文翻译，nullable；永远不覆盖 `quoted_text` 或 `comment`。

SQLite FTS5 方案可以新增一个 annotation FTS table（或将这三个字段纳入未来既有 Library FTS projection），每个 row 以 `paper_annotation.id` 为 rowid，查询时 join `paper_id -> papers` 和 `attachment_id -> paper_attachments`。FTS 索引不应重复复制整篇 paper 内容；Library Search 结果显示 annotation kind/page、短 quote、comment/translation 摘要，并可导航到 attachment/page/quadpoints。导入、编辑 comment、生成 translation 时增量更新 FTS；删除/失效 attachment 时使用 tombstone/FTS delete。

当前最新 `origin/main` 已包含 v15 `paper_attachments` linked-PDF relation（该 migration 来自基线 commit，本 spike 未创建或修改它）；当前没有现成 FTS，也没有 `paper_annotations`。因此这里只设计 annotation layer，不修改现有 `SCHEMA_VERSION`，不添加 annotation migration。

## Annotation identity strategy

`/NM` 是有用的 external identity，但不是充分条件：它是 optional、page-scoped，且不同 app 可能不写、重写或复制它。对象号/生成号、`/Annots` 数组顺序、绝对路径、文件 mtime 都不能作为 identity。

建议使用 versioned two-tier identity：

```text
primary key when NM exists:
  v1:nm:{attachment_id}:{page_index}:{external_annotation_id}

fallback fingerprint:
  sha256(canonical_json({
    version: 1,
    attachment_id,
    page_index,
    kind,
    quantized_quadpoints,
    normalized_quoted_text,
    normalized_comment
  }))
```

规则：

- `page_index` 用 0-based PDF page index，因为 `/NM` 只保证同页唯一。
- `quantized_quadpoints` 使用统一 page-space，按约 0.5–1.0 pt 量化，减少 app 重写浮点数造成的假 duplicate；原始坐标仍放 raw metadata。
- `normalized_quoted_text` 做 Unicode NFKC、whitespace collapse、casefold；展示层保留 extractor 的原始可证实文字。ligature/hyphen heuristic 不应覆盖原文。
- 有 `/NM` 时，comment/geometry 变化应更新同一 row，不应生成新 row；`/M` 只作为 modified metadata。
- 无 `/NM` 时先尝试 exact fallback fingerprint；若 comment 变化，再尝试同 attachment/page/kind、geometry+quote 高相似且唯一的候选，命中后更新 fingerprint；多个候选时不自动合并。
- 同一 `/NM` 在同一 page 重复出现时，标记 ambiguous 并退回 geometry/text fingerprint；不要 silently merge。
- `attachment_id` 必须参与 identity。同一 Paper 的两个不同 PDF attachment 即使内容相同，也不能共享 annotations。

这能覆盖“重复扫描同一 PDF 不产生 duplicate annotation”，也能在 relink 时保持 identity；但 PDF 被重排、重新排版或 annotation 被 flatten 后，任何 geometry-based identity 都只能做 best-effort。

## Relink strategy

annotation 只关联 `canonical paper_id + attachment_id`，绝不以 absolute path 作为业务外键。建议 attachment 层保存 stable id、当前 locator/path、`content_sha256`、byte size/page count 和最后扫描时间；这些是 attachment 元数据设计，不在本分支迁移。

1. relink 只更新 attachment locator/path，不改变 `paper_id` 或 `attachment_id`。
2. relink 后先算 SHA-256：hash 相同则直接认为内容相同，重复 scan 必须幂等。
3. hash 不同则重新枚举 annotations：先用 `attachment_id + page_index + /NM` 匹配，再用 fingerprint/fuzzy geometry+text 匹配。
4. 如果 page count、page rotation 或 text geometry 变化导致无法唯一匹配，保留旧 row 并标记 stale/unresolved，给用户选择，不把 annotation 自动迁移到“看起来最像”的 page。
5. 一个 Paper 可以有多个 attachment；Inspector 默认按 attachment 分组，避免同一页码来自两个 PDF 时混淆。
6. 文件被 flatten 后，原 annotations 不再存在；显示“source no longer contains editable annotation”，不要删除 CowPaper 已导入的历史 row。

## Proposed schema

只设计，不正式 migration。最小可实现模型如下；当前基线 v15 已提供 `paper_attachments`，因此 `paper_annotations.attachment_id` 应 FK 到现有 attachment identity 层，不以 path 代替。本 spike 不改变 v15。

```text
paper_annotations
------------------
id                         stable local row id
paper_id                  canonical paper FK
attachment_id             stable attachment FK; required
external_annotation_id    optional PDF /NM, page-scoped
kind                      highlight | underline | strikeout | text | freetext
page_index                zero-based PDF page index
color                     normalized color, nullable
quoted_text               source PDF text, nullable on extraction failure
comment                   /Contents or note text, nullable
translation               nullable AI Chinese translation; never overwrites source
author                    /T, nullable
created_at                PDF /CreationDate if present, nullable
modified_at               PDF /M, nullable
imported_at               local import timestamp
fingerprint               versioned SHA-256 fallback/idempotency key
extraction_status         extracted | no_text | scanned | encrypted | malformed | unsupported | missing_attachment
source_app                optional diagnostic hint, not identity
raw_metadata_json         Rect, QuadPoints, page size/rotation, Popup/IRT refs, raw dates/flags
```

`raw_metadata_json` is intentionally the single escape hatch for exact geometry and app-specific keys; it avoids prematurely promoting every PDF dictionary key into columns. If Inspector navigation becomes a hot path, `quadpoints_json` can later be promoted from this blob without changing identity semantics.

`created_at`/`modified_at` are nullable because PDF files often omit one or both. `imported_at` is local ingestion time. `translation` should carry only AI output associated with an already imported row; no AI call may create an annotation row.

## AI translation strategy

对于已存在且 extraction status 为 `extracted` 的 annotation，异步请求只接受 `{annotation_id, quoted_text, target_locale}`，结果写入 `translation`。UI 永远并列展示：

```text
Highlight · Page 3
quoted English text
我的批注
中文翻译
```

原始 `quoted_text` 和 `comment` 只读保留；重新翻译只替换 translation，不删除/覆盖 source。对 `no_text`、`scanned`、`malformed`、`unsupported`、`missing_attachment` 不调用翻译；AI 不得从 page image、comment 或 prompt 自己生成一个不存在的 annotation。

## Failure states

Inspector 需要让失败可见，而不是把 annotation 静默丢弃：

| 状态 | UI 建议 | 数据行为 |
|---|---|---|
| Annotation found but text extraction failed | `Highlight · Page 3` + “Quoted text unavailable” + comment + “无法从页面文字层定位” | 保留 row、quadpoints、comment、status；`quoted_text=NULL`。 |
| Scanned PDF | “Scanned PDF — no text layer” | 保留 annotation metadata；不调用 OCR，不生成 quote。 |
| Encrypted PDF | “PDF encrypted — password required” | 不创建猜测性 annotation；attachment scan 状态为 blocked，等待用户解锁/提供副本。 |
| Malformed annotation | “Malformed annotation” + page/type/object diagnostic | 保留可安全读取的 raw fields；坏 QuadPoints 不进入 quote/FTS。 |
| Unsupported annotation | “Unsupported annotation: /Ink /Stamp /Widget …” | 默认不进入 Annotation Search；可在 import summary 显示数量。 |
| Missing attachment | “Attachment unavailable / relink required” | 保留 `paper_id + attachment_id` row，停止重复 import，等待 relink。 |
| Flattened PDF | “Annotation appearance is flattened” | 不把画面像素当可编辑 annotation；既有 CowPaper row 不删除。 |

## Prototype

**PROTOTYPE: PASS**（受控 fixture；不是 production readiness 证明）。

实现文件：

- [`research/pdf_annotations/generate_fixture.py`](/Users/hiahiayue/Developer/CowPaper-annotation/research/pdf_annotations/generate_fixture.py)：生成 disposable synthetic PDF；没有用户数据。
- [`research/pdf_annotations/extract.py`](/Users/hiahiayue/Developer/CowPaper-annotation/research/pdf_annotations/extract.py)：`pypdf` 读取 `/Annots`，`pdfplumber` 读取 text geometry；只读输入，不保存 PDF。
- [`research/pdf_annotations/fixtures/annotation_fixture.pdf`](/Users/hiahiayue/Developer/CowPaper-annotation/research/pdf_annotations/fixtures/annotation_fixture.pdf)：含 Highlight、两个 QuadPoints 的多片段 Highlight、Underline、StrikeOut、Text、FreeText。

运行：

```bash
/Users/hiahiayue/.cache/codex-runtimes/codex-primary-runtime/dependencies/python/bin/python3 \
  research/pdf_annotations/generate_fixture.py
/Users/hiahiayue/.cache/codex-runtimes/codex-primary-runtime/dependencies/python/bin/python3 \
  research/pdf_annotations/extract.py \
  research/pdf_annotations/fixtures/annotation_fixture.pdf
```

观察到的关键输出：

```text
Highlight     quadpoints=1  quoted_text="Reliable quoted text comes from page geometry, not Contents."
Highlight     quadpoints=2  quoted_text="Multiple highlight fragments can be joined in reading order."
Underline     quadpoints=1  quoted_text="Multiple highlight fragments can"
StrikeOut     quadpoints=1  quoted_text="be joined in reading order."
Text          quadpoints=0  quoted_text=null  contents="Standalone text note"
FreeText      quadpoints=0  quoted_text=null  contents="FreeText is visible directly on the page."
```

fixture 渲染后的 input SHA-256 仍为 `40c85b14f6b43796f473d5cb379e0bdeaaf9ff9751682623df6175e92f23950c`，说明 extraction/render QA 没有修改 fixture input；本 spike 没有任何用户真实 PDF 输入。

## Implementation boundary

v0.3.0 的第一阶段建议：

- read-only import of embedded `/Annots` for the five target subtypes;
- PDFium-backed text geometry matching with explicit extraction status;
- paper/attachment identity、relink、idempotent import；
- Inspector Annotation View 和 quoted_text/comment/translation display；
- opt-in Library Search indexing for the three text fields；
- macOS Preview/Acrobat/Skim embedded export/Zotero embedded export fixtures；
- no built-in PDF Reader、no annotation authoring、no PDF write-back、no implicit OCR、no Skim xattr parser、no Zotero SQLite connector in first release。

真实实现前仍需建立跨 app fixture matrix（Preview、Acrobat、Skim embedded、Zotero export）、rotation/multi-column/ligature/hyphenation/scanned/encrypted/malformed cases，并对 macOS arm64/x86_64 与 Windows x64 的 PDFium runtime 做 CI smoke test。

## Final report

**PDF ANNOTATION STANDARD SUPPORT:** `/Annots` + `/Highlight` + `/Underline` + `/StrikeOut` + `/Text` + `/FreeText` metadata are understood; `/QuadPoints` is the authoritative text-markup geometry; `/Contents` is comment/text, not quote; `/Popup` and `/IRT` require relationship-aware import.

**PREVIEW COMPATIBILITY:** YES for editable embedded annotations saved/exported by Preview; flattened Print > Save as PDF is irreversible for extraction; no stable CowPaper identity guaranteed.

**ADOBE COMPATIBILITY:** YES for standard embedded annotations and explicit FDF/XFDF workflows; managed/cloud review may be external; `/NM` is best-effort only.

**SKIM COMPATIBILITY:** PARTIAL; default notes/highlights may live in xattrs/`.skim`/PDF Bundle, while “With Embedded Notes” output is importable standard PDF.

**ZOTERO COMPATIBILITY:** PARTIAL; Zotero-native annotations live in SQLite `itemAnnotations`, while exported embedded PDF annotations are importable; a future Zotero DB adapter is separate scope.

**HIGHLIGHT TEXT EXTRACTION:** **FEASIBLE / PARTIAL / NOT RELIABLE = PARTIAL**.

**RECOMMENDED PDF LIBRARY:** `pdfium-render`, optionally paired with a narrow read-only `lopdf` raw metadata pass.

**CROSS-PLATFORM:** **YES**, conditional on shipping and testing PDFium runtime binaries for macOS and Windows; not pure Rust.

**ANNOTATION IDENTITY STRATEGY:** `/NM + attachment_id + page_index` when present; otherwise versioned SHA-256 fingerprint over attachment/page/kind/quantized quadpoints/normalized quote/comment, with unique-candidate fuzzy update for comment edits.

**RELINK STRATEGY:** preserve canonical `paper_id + attachment_id`, update only locator/path, compare content SHA-256, reimport idempotently, mark ambiguous/stale instead of silently moving annotations.

**PROPOSED SCHEMA:** `paper_annotations` with the fields above; `translation` remains separate; `raw_metadata_json` stores exact geometry/relationship data; design only.

**SEARCH INTEGRATION:** FTS5/projection indexes `quoted_text`, `comment`, `translation`, joined by annotation id to canonical paper and attachment; failed quote is not indexed.

**AI TRANSLATION STRATEGY:** translate only an imported, existing `quoted_text`; save nullable `translation`; never overwrite source and never create annotations.

**PROTOTYPE:** **PASS** for controlled synthetic fixture; not a production compatibility claim.

**FORMAL MIGRATION CREATED:** **NO**（本 spike 未创建；基线已有的 v15 `paper_attachments` migration 未由本 spike 添加）。

**USER PDF MODIFIED:** **NO**.

**RISKS:** text layout/reordering, ligatures, hyphenation, multi-column and rotation ambiguity; no text layer/OCR; encryption; malformed PDFs; flattening; app-specific sidecars/databases; `/NM` instability; PDFium runtime packaging and API gaps for raw relationship fields; MuPDF AGPL/commercial licensing.

**RECOMMENDED PRODUCT SCOPE:** read-only embedded annotation import + Inspector + explicit failure states + optional FTS; exclude authoring, built-in reader, OCR, Skim native sidecar, Zotero DB connector from first implementation.

**RECOMMENDED VERSION:** **v0.3.0** for guarded product implementation; `v0.2.x` only for internal prototype.

**READY FOR IMPLEMENTATION:** **YES**, but only within the bounded read-only scope above and after the cross-app fixture/CI matrix is added.

**完成后状态：**本 spike 已停止；没有 merge main，没有开始正式 annotation 产品代码。
