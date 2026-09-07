---
version: alpha
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
  table-column-gap: "12px"
  inspector-label-column: "60px"
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

The Library has one global toolbar. It owns the page title, current Collection/Tag scope pills, the single Library Search box, status, and import/action controls. Do not add a second persistent search field to the sidebar or table header.

### Table

The table header is a low-contrast band with thin separators. Rows are compact, aligned to the same CSS grid as the header, and use ellipsis for constrained metadata. Keep Title as the primary column; Journal, Year, Authors, and Note remain secondary. Selected rows use the neutral selection color and retain readable metadata contrast.

### Inspector

The Inspector is a continuous metadata surface. Keep the serif paper title prominent, use short label/value rows for citation metadata, and group Citation, Library, Abstract, PDF, and Citation Format with thin rules and whitespace. Inline edit affordances stay hidden until row hover/focus, while links use the single accent color.

### Search

Use one Search box in the Library toolbar. Suggestions are lightweight and anchored to the box: recent/available scopes and matching Collection or Tag names only. Search must not replace the current sidebar hierarchy or introduce a large command palette.

### Selection, icons, and density

Use outline/neutral symbols in default states: paper, folder, and tag-dot markers are small supporting cues, not decoration. The accent is reserved for active navigation, links, focus, and drag feedback. Preserve the existing dense row rhythm and keep numeric counts tabular; never increase padding merely to make the Library resemble a card grid.

## Do's and Don'ts

- Keep one accent color per Library view and keep secondary actions neutral.
- Keep titles, metadata, counts, and Inspector labels aligned to their existing grid owners.
- Use truncation for dense table cells and retain the full value in the existing title/hover affordance.
- Give empty search results one clear recovery action: clear the query or scope.
- Do not introduce React, Tailwind, Radix, motion/react, a component framework, or a second design-token system; the surface is Vanilla TypeScript and CSS.
