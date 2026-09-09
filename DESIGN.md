---
version: rc2
name: CowPaper Library
description: Evidence-based design contract for CowPaper's macOS-style Library surface.
colors:
  background-main: "#ffffff"
  background-sidebar: "#f6f6f6"
  background-inspector: "#f5f5f5"
  text-primary: "#303030"
  text-secondary: "#898989"
  text-muted: "#aaaaaa"
  border-subtle: "#e9e9e9"
  accent-primary: "#287cff"
  focus-accent: "#3487ff"
  selection: "#dedede"
  selection-hover: "#d8d8d8"
  hover-neutral: "#f2f2f2"
typography:
  ui:
    fontFamily: '-apple-system, BlinkMacSystemFont, "PingFang SC", "Segoe UI", sans-serif'
    fontSize: "14px"
  sidebar-label:
    fontSize: "12px"
  table-title:
    fontSize: "12px"
    lineHeight: "16px"
    fontWeight: "400"
  table-meta:
    fontSize: "11px"
  inspector-title:
    fontFamily: "Georgia, 'Times New Roman', serif"
    fontSize: "16px"
    lineHeight: "1.35"
  inspector-body:
    fontFamily: "Georgia, 'Times New Roman', serif"
    fontSize: "12px"
    lineHeight: "1.7"
  section-label:
    fontSize: "10px"
    fontWeight: "500"
rounded:
  sidebar-item: "6px"
  table-row: "5px"
  relation-chip: "12px"
  inline-control: "5px"
spacing:
  sidebar-width: "168px"
  workspace-row-height: "54px"
  sidebar-content-top-gap: "18px"
  sidebar-section-gap: "24px"
  sidebar-section-item-gap: "6px"
  sidebar-item-height: "34px"
  toolbar-inline-gap: "10px"
  search-box-width: "430px"
  search-box-min-width: "250px"
  search-box-height: "28px"
  search-box-padding-inline: "7px"
  search-box-radius: "6px"
  search-popover-gap: "5px"
  search-popover-radius: "7px"
  search-item-padding: "6px 7px"
  table-column-gap: "12px"
  inspector-label-column: "60px"
search:
  mode: "all"
  placeholder: "搜索文库…"
  fields: "English title, Chinese title, authors, year, journal, Library Tags, note, English abstract, Chinese abstract"
  resultSemantics: "instant local feedback; commit on Enter or suggestion selection"
motion:
  librarySearch: "none"
  keyboardNavigation: "none"
  resultUpdate: "none"
  dropdown: "none"
  occasionalPanels: "transform and opacity only; 125-250ms ease-out; reduced-motion aware"
components:
  sidebar:
    background: "{colors.background-sidebar}"
    width: "{spacing.sidebar-width}"
    itemHeight: "{spacing.sidebar-item-height}"
    itemRadius: "{rounded.sidebar-item}"
  toolbar:
    background: "{colors.background-main}"
    height: "{spacing.workspace-row-height}"
  table:
    background: "{colors.background-main}"
    columnGap: "{spacing.table-column-gap}"
    titleFontSize: "{typography.table-title.fontSize}"
    metadataFontSize: "{typography.table-meta.fontSize}"
  inspector:
    background: "{colors.background-inspector}"
    labelColumn: "{spacing.inspector-label-column}"
    titleFontSize: "{typography.inspector-title.fontSize}"
    bodyFontSize: "{typography.inspector-body.fontSize}"
    currentTab: "metadata"
    futureTabs: "metadata, annotations"
    annotationStatus: "v0.3.0 direction only; not exposed in v0.2.1"
---

## Overview

CowPaper Library is a dense paper-management workspace: a narrow navigation rail, one continuous toolbar, a compact paper table, and a metadata Inspector. The visual language is quiet and neutral, with one restrained blue accent reserved for active, linked, focused, and drag-target states.

## Colors

Keep the table and toolbar white, the sidebar slightly gray, and the Inspector a separate light-gray reading surface. Use neutral text for content and metadata; use the accent for action affordances and current state. Selection is a neutral gray row treatment so the blue accent does not compete with paper titles.

## Typography

Use the system UI stack for navigation, table content, controls, and metadata labels. Use the serif Inspector title and abstract treatment already established by the Library surface for bibliographic reading content. Table titles stay one compact line when space is constrained; a Chinese title may occupy the second muted line without changing the column grid.

## Layout

The Library is a three-part composition: the `{spacing.sidebar-width}` sidebar, the `{spacing.workspace-row-height}` workspace toolbar, and the table/Inspector split. Keep the toolbar aligned to the main pane while the sidebar remains a continuous rail. The Inspector is resizable on wide windows and becomes a closable overlay at narrow widths; the table must not gain page-level horizontal overflow.

## Elevation & Depth

Use borders and surface changes to show structure. Keep the Library table flat and continuous. Reserve a small shadow for transient popovers or queued drop feedback; do not add card shadows to the persistent Inspector groups.

## Shapes

Use the existing navigation-item, compact-row/control, and relationship-chip radius tokens. Inspector groups are continuous sections separated by thin rules rather than rounded cards. Nested controls should not introduce a larger radius than their parent surface.

## Components

### Sidebar

Workspace tabs sit above the navigation items. Standard views, Collections, and Tags share the same row rhythm. Collection hierarchy is represented by indentation and folder symbols; Tags remain flat and use a small colored dot. Counts are right-aligned and tabular. Management actions remain quiet until the row is hovered or focused.

### Toolbar

The Library has one global toolbar. It owns the page title, the single Library Search box, status, and import/action controls. Collection/Tag scope tokens live inside the Search box; do not render a second persistent search field or duplicate scope chips in the toolbar, sidebar, table header, or Inspector. Toolbar inline controls use `{spacing.toolbar-inline-gap}`; the Search box uses the width/height/radius tokens above and remains the only persistent search surface.

### Table

The table header is a low-contrast band with thin separators. Rows are compact, aligned to the same CSS grid as the header, and use ellipsis for constrained metadata. Keep Title as the primary column; Journal, Year, Authors, and Note remain secondary. Selected rows use the neutral selection color and retain readable metadata contrast.

### Inspector

The Inspector is a continuous metadata surface. Keep the serif paper title prominent, use short label/value rows for citation metadata, and group Citation, Library, Abstract, PDF, and Citation Format with thin rules and whitespace. Inline edit affordances stay hidden until row hover/focus, while links use the single accent color. The current product contract has one visible `元数据` tab. The v0.3.0 direction reserves a sibling `标注` tab for paper-linked annotations, but v0.2.1 must not expose an empty tab, annotation extraction, or a migration.

### Search

Use one Search box in the Library toolbar. Search has one mode: `all`, covering English title, Chinese title, authors, year, journal, Library Tags, note, English abstract, and Chinese abstract. DOI, URL, publisher, volume, issue, and pages remain metadata only and are excluded from full-text matching. Suggestions are lightweight and anchored to the box: matching Collection or Tag names, matching Paper rows, followed by one `在当前范围搜索“…”` action. Do not offer Quick/Metadata/Content mode switching or a large command palette. Empty-query focus remains quiet; typed-query filtering and keyboard navigation update without animation. Zero-count Tags remain visible and dimmed.

### Suggestion dropdown

The dropdown is a tokenized, trigger-owned surface: `{spacing.search-popover-gap}` below the Search box, `{spacing.search-popover-radius}` radius, and `{spacing.search-item-padding}` item padding. It is limited to Collection, Tag, Paper, and one action group, keeps the active row adjacent to the input, closes on outside click or `Escape`, and never changes layout or steals focus. No enter/exit animation is required for this high-frequency control; if a future product surface earns motion, it must use a trigger-aware origin, `transform`/`opacity` only, a sub-300ms ease-out curve, and `prefers-reduced-motion` handling.

### Motion policy

Search input, IME composition, `⌘F`, arrow-key navigation, `Enter`, `Escape`, scope changes, and result/Inspector updates are instant. Do not animate filtering, row replacement, dropdown open/close, focus movement, or selection. The Library is a crisp information surface: motion is reserved for occasional, spatially meaningful panels only, with exact-property transitions, no `transition: all`, no layout-property animation, and no keyframe restarts on rapid interactions. Respect reduced motion by removing movement while retaining useful color/opacity feedback.

### Selection, icons, and density

Use outline/neutral symbols in default states: paper, folder, and tag-dot markers are small supporting cues, not decoration. The accent is reserved for active navigation, links, focus, and drag feedback. Preserve the existing dense row rhythm and keep numeric counts tabular; never increase padding merely to make the Library resemble a card grid.

## Do's and Don'ts

- Keep one accent color per Library view and keep secondary actions neutral.
- Keep titles, metadata, counts, and Inspector labels aligned to their existing grid owners.
- Use truncation for dense table cells and retain the full value in the existing title/hover affordance.
- Give empty search results one clear recovery action: clear the query or scope.
- Keep the Search contract single-mode and all-field; scope is expressed by Collection/Tag state, not by a mode selector.
- Do not introduce React, Tailwind, Radix, motion/react, a component framework, or a second design-token system; the surface is Vanilla TypeScript and CSS.
- Do not expose the future Inspector `标注` tab before annotation data exists; do not implement annotation extraction or migrations as part of v0.2.1.
