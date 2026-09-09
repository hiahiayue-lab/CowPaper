---
version: rc3
name: CowPaper
description: Evidence-based full-app design contract for CowPaper's macOS-style Tauri workspace.
colors:
  background-app: "#f5f6f8"
  background-main: "#ffffff"
  background-sidebar: "#f6f6f6"
  background-inspector: "#f5f5f5"
  surface-raised: "#ffffff"
  surface-subtle: "#f4f6f8"
  surface-control: "#f4f7fa"
  surface-hover: "#f2f2f2"
  surface-active: "#ececec"
  surface-selection: "#dedede"
  surface-selection-hover: "#d8d8d8"
  surface-drop: "#eef5fd"
  text-primary: "#303030"
  text-secondary: "#898989"
  text-muted: "#aaaaaa"
  text-placeholder: "#aaaaaa"
  text-on-accent: "#ffffff"
  border-subtle: "#e9e9e9"
  border-default: "#e3e6ea"
  border-control: "#dedede"
  border-divider: "#f0f0f0"
  accent-primary: "#287cff"
  accent-hover: "#1d4ed8"
  focus-accent: "#3487ff"
  state-info: "#2563eb"
  state-success: "#16a34a"
  state-warning: "#d97706"
  state-danger: "#dc2626"
  state-info-surface: "#eff6ff"
  state-success-surface: "#f0fdf4"
  state-warning-surface: "#fffbeb"
  state-danger-surface: "#fef2f2"
typography:
  ui:
    fontFamily: '-apple-system, BlinkMacSystemFont, "PingFang SC", "Segoe UI", sans-serif'
    fontSize: "14px"
    lineHeight: "20px"
    fontWeight: "400"
  reading:
    fontFamily: "Georgia, 'Times New Roman', serif"
    fontSize: "12px"
    lineHeight: "20px"
    fontWeight: "400"
  scale:
    caption:
      fontSize: "10px"
      lineHeight: "14px"
      fontWeight: "500"
    meta:
      fontSize: "11px"
      lineHeight: "16px"
      fontWeight: "400"
    body-compact:
      fontSize: "12px"
      lineHeight: "16px"
      fontWeight: "400"
    body:
      fontSize: "13px"
      lineHeight: "20px"
      fontWeight: "400"
    ui:
      fontSize: "14px"
      lineHeight: "20px"
      fontWeight: "400"
    section:
      fontSize: "14px"
      lineHeight: "20px"
      fontWeight: "650"
    paper-title:
      fontSize: "15px"
      lineHeight: "21px"
      fontWeight: "650"
    title:
      fontSize: "16px"
      lineHeight: "22px"
      fontWeight: "600"
    display:
      fontSize: "18px"
      lineHeight: "22px"
      fontWeight: "650"
    mono:
      fontFamily: "ui-monospace, SFMono-Regular, Menlo, monospace"
      fontSize: "11px"
      lineHeight: "16px"
      fontWeight: "400"
  sidebar-label:
    fontSize: "12px"
    lineHeight: "16px"
    fontWeight: "500"
  table-title:
    fontSize: "12px"
    lineHeight: "16px"
    fontWeight: "400"
  table-meta:
    fontSize: "11px"
    lineHeight: "16px"
    fontWeight: "400"
  inspector-title:
    fontFamily: "Georgia, 'Times New Roman', serif"
    fontSize: "16px"
    lineHeight: "1.35"
    fontWeight: "700"
  inspector-body:
    fontFamily: "Georgia, 'Times New Roman', serif"
    fontSize: "12px"
    lineHeight: "1.7"
    fontWeight: "400"
  section-label:
    fontSize: "10px"
    lineHeight: "14px"
    fontWeight: "500"
rounded:
  none: "0px"
  micro: "4px"
  table-row: "5px"
  control: "6px"
  sidebar-item: "6px"
  inline-control: "5px"
  card: "10px"
  relation-chip: "12px"
  group: "12px"
  modal: "16px"
  pill: "999px"
spacing:
  scale:
    hairline: "1px"
    micro: "2px"
    tight: "4px"
    control: "6px"
    compact: "8px"
    inline: "10px"
    row: "12px"
    inset: "14px"
    content: "16px"
    rail-gap: "18px"
    content-wide: "20px"
    section: "24px"
    reading: "28px"
    table-header: "27px"
    table-row: "30px"
    table-row-with-secondary-line: "39px"
    sidebar-item: "34px"
    workspace-row: "54px"
  sidebar-width: "168px"
  sidebar-content-top-gap: "18px"
  sidebar-section-gap: "24px"
  sidebar-section-item-gap: "6px"
  sidebar-item-height: "34px"
  toolbar-inline-gap: "10px"
  search-box-width: "430px"
  search-box-min-width: "250px"
  search-box-height: "28px"
  search-box-padding-inline: "7px"
  search-popover-gap: "5px"
  search-popover-radius: "7px"
  search-item-padding: "6px 7px"
  table-column-gap: "12px"
  inspector-width: "300px"
  inspector-width-min: "300px"
  inspector-width-max: "560px"
  inspector-label-column: "60px"
borders:
  subtle: "1px solid {colors.border-subtle}"
  default: "1px solid {colors.border-default}"
  control: "1px solid {colors.border-control}"
  focus: "2px solid {colors.focus-accent}"
  divider: "1px solid {colors.border-divider}"
surfaces:
  app: "{colors.background-app}"
  main: "{colors.background-main}"
  sidebar: "{colors.background-sidebar}"
  inspector: "{colors.background-inspector}"
  raised: "{colors.surface-raised}"
  transientShadow: "0 8px 24px #00000014"
  persistentShadow: "none"
search:
  mode: "all"
  placeholder: "搜索文库…"
  fields: "English title, Chinese title, authors, year, journal, Library Tags, note, English abstract, Chinese abstract"
  excludedMetadata: "DOI, URL, publisher, volume, issue, pages"
  resultSemantics: "instant local feedback; commit on Enter or suggestion selection"
motion:
  highFrequency: "none"
  librarySearch: "none"
  keyboardNavigation: "none"
  resultUpdate: "none"
  dropdown: "none"
  occasionalPanels: "transform and opacity only; 125-250ms ease-out; reduced-motion aware"
  progress: "instant or transform scaleX; never width transition"
accessibility:
  focus: "visible focus ring for every keyboard-operable control"
  contrast: "state is never conveyed by color alone"
  reducedMotion: "remove movement while retaining state and progress information"
  target: "desktop pointer and keyboard; compact controls retain a visible focus target"
components:
  windowChrome:
    height: "{spacing.scale.workspace-row}"
    nativeTitleBar: "preserve OS-owned title-bar/traffic-light space; do not fake controls"
  sidebar:
    background: "{colors.background-sidebar}"
    width: "{spacing.sidebar-width}"
    itemHeight: "{spacing.sidebar-item-height}"
    itemRadius: "{rounded.sidebar-item}"
    sectionGap: "{spacing.sidebar-section-gap}"
  toolbar:
    background: "{colors.background-main}"
    height: "{spacing.scale.workspace-row}"
    inlineGap: "{spacing.toolbar-inline-gap}"
  search:
    width: "{spacing.search-box-width}"
    minWidth: "{spacing.search-box-min-width}"
    height: "{spacing.search-box-height}"
    radius: "{rounded.control}"
    suggestionRadius: "{spacing.search-popover-radius}"
  table:
    background: "{colors.background-main}"
    columnGap: "{spacing.table-column-gap}"
    titleFontSize: "{typography.table-title.fontSize}"
    metadataFontSize: "{typography.table-meta.fontSize}"
    selectedRow: "{colors.surface-selection}"
  inspector:
    background: "{colors.background-inspector}"
    width: "{spacing.inspector-width}"
    labelColumn: "{spacing.inspector-label-column}"
    titleFontSize: "{typography.inspector-title.fontSize}"
    bodyFontSize: "{typography.inspector-body.fontSize}"
    currentTab: "metadata"
    futureTabs: "metadata, annotations"
    annotationStatus: "v0.3.0 direction only; not exposed in v0.2.1"
  settings:
    contentMaxWidth: "680px"
    groupRadius: "{rounded.group}"
  dialog:
    radius: "{rounded.modal}"
    maxWidth: "440px"
---

## Overview

CowPaper is a desktop literature workspace with two related modes: Discovery for finding, evaluating, and saving papers, and Library for organizing saved papers and their PDF attachments. The visual system is quiet, dense, and macOS-like: neutral surfaces carry the information, one restrained blue accent carries active and actionable state, and shadows are reserved for transient surfaces.

The contract applies to the whole app. The Library's RC2/RC3 decisions remain normative: a 168px navigation rail, a 54px workspace row, one all-field Library Search box, compact table rows, a continuous Inspector, and no empty Annotation tab.

## Foundations

### Colors and roles

Use semantic roles from the frontmatter instead of feature-specific colors. The app background is `background-app`; persistent content is `background-main`; the navigation rail and Inspector are their own quiet surfaces. `accent-primary` is reserved for active navigation, links, focus, selected controls, and local drag feedback. Selection in the Library is neutral gray so a selected paper does not look like a link.

State colors are semantic and must be paired with text, an icon, or an action: info for work in progress, success for a completed command, warning for partial or missing source data, and danger for an actionable failure. Do not use a colored dot or opacity change as the only state signal.

### Typography

Use the system UI stack for navigation, buttons, filters, table content, metadata, settings labels, and status text. Use the reading serif only for bibliographic reading content in the Inspector: the paper title and abstract. The named type scale is closed; a new size requires a documented component exception.

Use tabular numerals for counts, years, progress, and scores. Truncate dense table cells with ellipsis while preserving the full value through the existing title/hover affordance. A Chinese title may occupy the second muted line without changing the table grid.

### Spacing, shapes, borders, and surfaces

Use the spacing scale for all new layout work. The primary rhythm is 6px control padding, 8px compact grouping, 10–12px inline/row gaps, 16px content inset, 24px section separation, and 34px navigation rows. Persistent surfaces use borders and whitespace; transient popovers, menus, drop queues, and dialogs may use the single transient shadow. Inspector groups are continuous sections separated by thin rules, not cards.

Use the smallest radius that communicates the component: 5–6px for controls and rows, 10–12px for cards/groups, 16px for dialogs, and 999px only for semantic chips or pills. Nested controls must not introduce a larger radius than their parent surface.

## Window chrome and navigation rail

The OS owns the native title-bar/traffic-light area. CowPaper must preserve that space and the continuous sidebar background; it must not draw fake traffic-light buttons. The app chrome below it has one 54px workspace row across Discovery and Library. The row owns the workspace switcher, view title, Search when Library is active, background-work status, and global actions.

The rail is 168px wide on the desktop baseline. Workspace tabs sit above navigation. Discovery sections use the same 34px row rhythm as Library standard views, Collections, and flat Library Tags. Collection hierarchy is expressed by indentation and folder symbols; tags remain flat and use a small color dot. Counts align to a 30px tabular-number column. Management affordances are quiet until hover or keyboard focus.

## Discovery

Discovery owns 今日, 历史, 稍后看, 期刊, 研究兴趣, and 活动. Its paper cards use the raised surface, a 10px radius, 14–16px inset, a 15px paper title, compact metadata, and a single primary next action. The abstract, Chinese title, AI summary, journal/collection badges, score, and missing/partial source notice have a stable order so cards do not jump when optional data appears.

Recommendations are a ranked reading queue, not a second Library. Show the daily status and the 推荐/缺摘要 segment near the page heading; keep score as supporting metadata. A missing abstract remains a paper row with a warning and a concrete recovery action. Recommendation history uses the same paper-card and empty-state contract and must not imply that a historical snapshot is live.

The All Papers filters are metadata controls, not a second global Search. Keep filter groups compact and wrap them without page-level horizontal overflow. Journals and Research Interests use the same card/control language, with subscription or tag configuration changes acknowledged inline near the changed control.

## Toolbar and background-work status

The toolbar has one responsibility boundary. Search is persistent only in Library; Discovery keeps its title and global actions without a disabled or duplicate Library field. The toolbar work-status control is the compact entry point for sync and AI background work. Its popover gives the detail and a link to Activity. A transient completion/error notice may appear as a status toast, but it must not duplicate an always-visible background-work status.

Toolbar buttons use the shared control height and neutral ghost treatment. Use the blue primary treatment only for the one next action in a local flow, such as saving a settings change or starting an explicitly requested analysis. Icon-only actions require an accessible label and a visible focus state.

## Library Search

Library Search has one mode: `all`. It matches English title, Chinese title, authors, year, journal, Library Tags, note, English abstract, and Chinese abstract. DOI, URL, publisher, volume, issue, and pages remain metadata and are not full-text fields. Collection and Tag scope is expressed by removable tokens inside the Search box; do not add a second mode selector or duplicate scope chips elsewhere.

Typing, IME composition, `⌘F`, arrow-key navigation, token changes, `Enter`, `Escape`, and result replacement are instant. Empty-query focus is quiet. The anchored suggestion surface contains Collection, Tag, Paper, and one current-scope search action; zero-count Tags stay visible but dimmed. The dropdown does not animate, steal focus, or change the page layout.

## Paper Table and attachments

The table is the primary Library surface: white, flat, compact, and aligned to one grid shared by the header and rows. The header is 27px; a normal row is 30px and a row with a secondary Chinese-title line is 39px. Title is primary; Note, Journal, Year, and Authors are secondary. Use a neutral selected row (`surface-selection`) and a lighter hover; do not turn selection into a blue card.

Column visibility, ordering, and resizing are table-local actions. Keep Title visible, keep years tabular, and preserve the no-page-horizontal-overflow rule. At narrow widths the Inspector becomes a closable overlay and secondary columns may be hidden according to the existing responsive contract.

PDF attachments are subordinate children of a paper, not separate papers. A managed, linked, missing, processing, complete, or failed attachment state must be named in text. Drag/drop feedback stays local to the row or Library list pane. The queue may expose a progressbar, but progress feedback is instant or transform-based; never animate layout with a `width` transition.

## Inspector

The Inspector is a continuous reading surface with a serif paper title and abstract, short label/value citation rows, and thin rules separating Citation, Library, Abstract, PDF, and Citation Format. Empty metadata uses muted text plus a concrete edit/add action when one exists. Inline edit affordances remain quiet until hover or keyboard focus. Attachment actions distinguish Open, Show Location, Relink, and Detach.

Only `元数据` is visible in v0.2.1. Do not render, reserve, or advertise an empty `标注`/Annotation tab. The future contract is documented below so implementation can remain stable without exposing unfinished UI.

### Future Inspector metadata/annotation contract

The Metadata tab remains the owner of canonical citation metadata, effective/personal Library overrides, relations, abstract language, and attachment provenance. A future Annotation tab is a sibling view owned by the selected paper, not by the global Library or the current search query. Its conceptual record is:

```ts
type Annotation = {
  id: string;
  paperId: string;
  quote?: string;
  locator?: { page?: number; paragraph?: string; anchor?: string };
  note: string;
  createdAt: string;
  updatedAt: string;
};
```

This is a future UI/data contract, not a v0.2.1 runtime schema. When the feature exists, render the tab only when the annotation capability and provider are available; a zero-record state must have one create/import action and must never be represented by a permanently empty tab in this release. Annotation selection must not change paper identity, canonical metadata, or attachment ownership. No annotation extraction, persistence, migration, or UI belongs in v0.2.1.

## Settings, collections, tags, and status

Settings is a focused form surface with a 680px content maximum. Group Update, DeepSeek, AI Analysis, Abstract, PDF File Library, and Sync into named sections. Use two-column label/control rows on wide windows and stack them on narrow windows. Keep destructive actions such as Delete Key and Reset visibly secondary and explicitly named. API-key, updater, and PDF-file messages stay next to the owning control; never rely on the bottom toast as the only explanation.

Collections are structural containers and use folder symbols, hierarchy, additive membership, and counts. Library Tags are flat semantic labels and use a dot plus text; their color is supplemental. Relation chips are compact, removable, and truncatable. Discovery collection badges remain lower emphasis than AI tags and never carry an AI score.

AI status text is the source of truth: `等待摘要`, `待分析`, `排队中`, `正在分析`, `已分析`, and `AI 分析失败`. Pair status text with the appropriate semantic role and an available next action. Partial abstracts use warning treatment, not failure red; a failed AI run uses danger treatment and a retry action; completed work keeps the paper result visible.

## Empty, loading, error, and success states

Every data region follows the same state order: explain what is absent or happening, preserve the surrounding context, then give one clear next action when one exists.

| State | Treatment | Example action |
| --- | --- | --- |
| Empty | muted copy on the owning surface; no decorative illustration required | save a paper, clear a filter, add a journal, or create a tag |
| Loading | inline `读取中…`/`分析中` or a live progress label; retain heading and controls | pause/stop a background task when supported |
| Partial or missing | warning role with the retained record and source explanation | `重新获取摘要`, choose a PDF, or fill the missing field |
| Error | danger text adjacent to the failed control plus a retry/recovery action | retry, relink, test connection, or inspect Activity |
| Success | brief inline confirmation or status toast; keep the resulting data in place | continue reading; do not leave a permanent success card |
| Disabled | lower emphasis plus `disabled` semantics; never only opacity | explain prerequisite in nearby helper text |

## Accessibility

All keyboard-operable controls need a visible `:focus-visible` ring using `focus-accent`, including icon buttons, segmented controls, row actions, resizers, menu items, relation controls, and modal buttons. Do not remove the browser outline without replacing it. Use real buttons and inputs, stable accessible names, `aria-pressed`/`aria-selected`/`aria-expanded`/`aria-busy`/`role=progressbar` where the state exists, and a single live region for command feedback.

Focus must remain predictable: opening a popover keeps focus in its owning control group, `Escape` closes and returns focus to the trigger, and opening a dialog moves focus into the dialog while closing restores the invoker. Error text must be adjacent to its field and associated where practical. Color, opacity, or a dot may reinforce state but cannot be the only carrier of meaning.

## Motion

CowPaper is a high-frequency desktop information surface. Search, IME, keyboard navigation, filtering, row replacement, selection, scope changes, and Inspector updates have no motion. Occasional spatial panels may use only `transform` and `opacity`, a 125–250ms ease-out, and a trigger-aware origin. Do not use `transition: all`, `ease-in`, layout-property animation, keyframe restarts, or spring motion for functional search.

The reduced-motion mode removes movement and preserves state, focus, and progress information. A progress indicator may update instantly or use `transform: scaleX(...)`; it must not animate `width`. No visual effect should delay an input path or make a user wait for a result that is already available.

## Do's and Don'ts

- Keep one accent role per view and keep secondary actions neutral.
- Keep titles, metadata, counts, filters, and Inspector labels aligned to their owning grid.
- Prefer borders, separators, and surface changes over persistent shadows.
- Give every empty state one useful next action when one is possible.
- Keep Library Search single-mode and all-field; keep scope in tokens.
- Keep the future Annotation contract documented but the v0.2.1 UI metadata-only.
- Do not introduce React, Tailwind, Radix, Motion, or a second design-token system; the current surface is Vanilla TypeScript and CSS.
- Do not add gradients, glow, fake native window controls, a second persistent Search field, or an empty Annotation tab.
