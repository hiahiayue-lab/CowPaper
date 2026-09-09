# CowPaper v0.2.1 Library Search + UI Design System

Status: RC2 design-engineering audit applied; implementation and design contract are aligned at the commit recorded below.

## Source and method

- Audited surface: the existing CowPaper Library workspace at `app/index.html`, `app/src/main.ts`, and `app/src/styles.css`.
- Baseline: `8eacbffc5bb12b83fc54a2ccc8b2da3602c6ba97` (`v0.2.1` RC2 worktree).
- Emil Skills source: `https://github.com/emilkowalski/skills`.
- Emil Skills source commit: `d23d7f88a2e21c9e4b1418c7abe420f5c1052ba7` (current `main` at audit time).
- Emil Skills fetched/read outside CowPaper through browser retrieval of the public README, the full `emil-design-eng`, `apple-design`, `review-animations`, and `improve-animations` skill files, plus the referenced `STANDARDS.md`, `AUDIT.md`, and `PLAN-TEMPLATE.md`; no repository, scripts, or skill files were vendored or executed.
- `EMIL_DESIGN_ENG_USED`: yes; applied frequency-first motion decisions, exact-property transitions, high-frequency keyboard instantness, and token/spacing review.
- `APPLE_DESIGN_USED`: yes; applied response/latency, spatial ownership, focus continuity, and reduced-motion principles; no spring was introduced because Library search is not a gesture surface.
- `REVIEW_ANIMATIONS_USED`: yes; reviewed existing transitions for purpose, frequency, origin, performance, and reduced-motion coverage; the Search dropdown is intentionally static.
- `IMPROVE_ANIMATIONS_USED`: yes; used its read-only recon/audit/vetting method and eight-category motion audit; source changes were limited to the requested implementation and design contract, not a separate animation plan.
- `BASELINE_UI_REVIEWED`: yes; applied only stack-neutral guidance below and preserved Vanilla TypeScript/CSS.

## Baseline UI applicability

Applicable to CowPaper:

- Preserve hierarchy, alignment, spacing, and dense-UI clarity before adding visual novelty.
- Use `tabular-nums` for counts and years; this already exists for `.nav-count` and should extend to search-result counts.
- Truncate long labels in constrained table/sidebar cells while keeping the full value available through the existing title/hover affordance.
- Keep one restrained accent per view, neutralize secondary actions, and use borders for structure rather than decorative glow.
- Keep routine interaction feedback short and local; do not add animation for search filtering unless explicitly requested.
- Keep empty states actionable and keep selection/dropdown feedback adjacent to the originating control.

Not applicable and intentionally excluded:

- Tailwind defaults, `cn`, `clsx`, `tailwind-merge`, `tw-animate-css`, `motion/react`, `Base UI`, React Aria, Radix, `h-dvh`, or React-specific render guidance. CowPaper is a Tauri app with Vanilla TypeScript/CSS and no framework dependency.
- The 44px touch-target rule is not a desktop macOS sizing contract for this dense Library surface; preserve the existing mouse/keyboard affordance sizes and verify focus visibility in the existing CSS model.
- Do not add animations, gradients, glow, skeleton systems, or new elevation to satisfy a generic baseline rule.

## Design language

- Audited surface: Library workspace only; Discovery, Settings, and recommendation cards are out of scope except for shared workspace chrome.
- Design sources: the effective Library rules at the end of `app/src/styles.css`, Library DOM in `app/index.html`, render paths in `app/src/main.ts`, `docs/qa/ui-rc3/{README.md,layout.json,interactions.json}`, and the accepted visual evidence in `docs/qa/ui-rc3/library-1512x982.png`.
- Documented decisions: RC3/RC4/RC5 comments and QA evidence establish a narrow 168px rail, continuous panels, compact rows, gray selection, thin separators, a resizable Inspector, and no document/list horizontal overflow.
- Governing owners and consumers: `.workspace-nav` owns Sidebar rhythm; `body.library-workspace .topbar` owns the unified toolbar; `.library-table-head` and `.library-paper-row` share `--library-columns`; `.library-inspector` and `.inspector-group` own metadata grouping; `renderLibraryNavigation`, `renderLibraryFacets`, `renderLibrary`, and `renderLibraryInspector` own Library composition.
- Explicit exceptions: None documented.

## Verified current surface

### Sidebar and Collection/Tag hierarchy

- `app/index.html:12-34` puts workspace tabs, standard Library views, Collection navigation, and Tag navigation in one rail.
- The effective CSS uses a 168px sidebar, a 54px workspace row, 34px navigation items, 24px section gaps, 6px item radius, and 12px sidebar labels.
- `renderLibraryNavigation()` recursively renders Collection children with 14px depth increments and a folder symbol. Tags are intentionally flat, use a 6px dot, and expose counts plus hover-only management actions.
- Current risk: `libraryTagFacets.map(...)` means the rendered Tag list is driven by returned facets. A zero-count Tag can disappear, which conflicts with the v0.2.1 requirement that it remain visible but dimmed.

### Unified toolbar and Search placement

- `app/index.html:37-63` has one `.topbar` with title/facet ownership on the left and Library actions/status on the right.
- The effective Library toolbar is fixed to the main pane at 54px and begins at the 168px sidebar edge.
- `.library-search` styles exist in `app/src/styles.css`, but there is no Library Search input in the current `app/index.html` or `main.ts` render path. This is an orphaned style, not an implemented search surface.
- The plan therefore adds one Search box to the existing toolbar slot. No sidebar search and no second table-header search are proposed.

### Paper Table

- `renderLibrary()` uses one `--library-columns` grid for the header and each paper row; the table currently shows Title, Note, Journal, Year, and Authors.
- Effective CSS sets a 27px header, 30px compact rows without a Chinese title, 39px rows with a Chinese title, 12px column gaps, and 12px primary title / 11px metadata type sizes.
- QA measured a 168px sidebar, a 300px Inspector, and no list/document horizontal overflow at 1440, 1512, and 1536px widths. At 740px Note hides; at 600px Authors hides.
- Selected rows are neutral gray (`#dedede`, hover `#d8d8d8`), matching the supplied Library evidence; keep search matches inside that same row treatment instead of adding a second accent.

### Inspector

- `renderLibraryInspector()` renders Metadata, Citation, Library, Abstract, PDF, and Citation Format groups.
- Effective Inspector styling is a continuous `#f5f5f5` surface, 60px label column, 10px section labels, 11px metadata values, a Georgia 16px title at 1.35 line-height, and Georgia 12px abstract text at 1.7 line-height.
- Grouping is done by thin top borders and spacing, not cards. Search selection should update the existing Inspector only after a row is selected; do not introduce a search-specific Inspector variant.

### Typography, spacing, density, alignment, selection, and suggestions

- Typography is system UI for chrome/table and serif for bibliographic reading content. Numeric counts/years are the tabular data lane.
- Alignment is grid-owned: header and rows share columns; sidebar counts are right aligned; Inspector label/value rows use the same label column.
- Density is compact and deliberate. Search controls must fit the existing 54px toolbar without changing the table baseline.
- Existing relation pickers use an 11px search input and a short option list; this is the local exemplar for a light dropdown, but the global Library Search must remain simpler and broader.

## Findings

| # | Problem | Evidence | Proposed change | Scope | Confidence |
| --- | --- | --- | --- | --- | --- |
| 1 | Library has no implemented Search box even though a `.library-search` style exists. | `app/index.html` contains no `.library-search`; `app/src/main.ts` has no Library query state or render path; the style is only present around `app/src/styles.css:983-986` and later overrides. | Add one query input to the existing unified Library toolbar and make it the sole persistent Library Search surface. | `app/index.html`, `app/src/main.ts`, `app/src/styles.css` | High |
| 2 | Scope was split across Sidebar rows and toolbar facet pills without a single visible query/scope model. | Sidebar owns standard views, recursive Collections, and flat Tags; the Search box now owns removable Collection/Tag tokens. | Keep scope tokens inside the Search box while preserving the Sidebar as navigation; do not render external duplicate chips. | Library toolbar, scope state/rendering, suggestion dropdown | High |
| 3 | Zero-count Tags are not guaranteed to remain visible, so hierarchy/selection can jump after filtering. | `renderLibraryNavigation()` renders `libraryTagFacets.map(...)`; facet data is count-driven, while the requested v0.2.1 behavior requires zero-count Tags visible/dimmed. | Render all Library Tags from `libraryTags`, join counts when present, keep zero-count rows visible with muted text/dot and disabled-looking count, while leaving them selectable for future scope changes only if the product state supports it. | Sidebar Tag renderer and its count presentation | High |

## Improve first

Implement the single toolbar Search box and its scope model first. It establishes the interaction owner for all later filtering, keeps the current three-pane hierarchy intact, and avoids the highest-risk regression: two competing search fields with different semantics.

## v0.2.1 implementation plan

### 1. One Search Box

Add a single compact search control inside `.topbar-leading`, next to the current Library title and before the right-side status/actions at wide widths, and collapse under the existing responsive constraints. Do not place a persistent search field in the Sidebar, table header, or Inspector.

Implemented UI contract:

- Placeholder: `搜索文库…`.
- Search icon is a neutral supporting glyph; the clear affordance appears only when the query is non-empty.
- `⌘F` focuses the same control; `Escape` clears the query when the box is focused and closes the suggestion list when it is open.
- Query updates are local and immediate for UI feedback; debounce only if the executing agent proves the backend list command needs it.
- There is one search mode, `all`. The query searches the Library result set across effective English title, Chinese title, authors, year, journal/source, Library Tags, notes, and abstracts. Publisher, DOI, URL, volume, issue, and pages remain display metadata only and are excluded from full-text matching. Do not search Discovery or mutate canonical Paper identity.
- Preserve the existing `libraryView`, Collection scope, Tag scope, selected row, column widths, and Inspector width across query changes.

### 2. Search scopes

Keep scope explicit in Sidebar navigation and inside Search Box tokens; do not add a mode selector, a second toolbar control, or an external duplicate chip row. The default is `All Library`. Available scopes:

1. `All Library`: search all papers in the Library.
2. `Current view`: search within All / Recent / Unfiled when one of those built-in views is active.
3. `Current Collection`: search within the selected Collection, including the existing parent/child scope semantics supplied by the backend.
4. `Current Tag(s)`: search within the active Tag selection. Preserve the current AND semantics for multiple Tags; do not reimplement Collection+Tag filtering in the browser.

When a Collection or Tag is active, render it as a removable token inside the Search Box. There is no external duplicate Collection/Tag chip row; removing a token changes only that dimension and leaves other tokens and text intact.

### 3. Light suggestion/dropdown

The dropdown is a small anchored surface below the Search box, not a command palette. It contains at most four groups:

- `文集`: matching Collection names, with folder icon and count where available.
- `标签`: matching Tag names, with dot and count; zero-count Tags are still listed but muted/dimmed.
- `论文`: matching Library Paper rows; selecting one locates the existing canonical row and never copies its title into query text.
- `操作`: one `在当前范围搜索“…”` action; Quick/Metadata/Content mode actions are excluded.

Behavior:

- Empty query + focus: stay quiet until text is entered; do not dump papers or every Collection/Tag.
- Non-empty query: show matching Collection/Tag/Paper names first, then one small `在当前范围搜索“…”` action. Selecting that action commits the query without changing the scope.
- No matches: show `没有匹配的范围或标签` and one clear `清除搜索`/`返回全部文献` action, matching the baseline empty-state rule.
- Close on outside click or Escape; preserve focus ring and do not animate the dropdown. Input, IME composition, keyboard navigation, and result/Inspector updates remain instant.
- Keyboard navigation must use the existing Vanilla TS event delegation pattern; no React/Radix primitive.

### 4. Collection/Tag scope and zero-count Tags

Keep Collection hierarchy recursive and Tag hierarchy flat. The search UI must not flatten Collections into tags or create a second hierarchy.

- Render Collection rows from `libraryCollections`; use the existing `parentId` recursion and indentation.
- Render Tag rows from the complete `libraryTags` list, joining `libraryTagFacets` counts by `tag.id`.
- A zero-count Tag remains in the Sidebar and Search suggestions with its normal name/dot but muted label/count and no accent active state unless it is the current selected scope.
- Do not hide zero-count Tags when the query is empty.
- If a zero-count Tag is selected, preserve the existing backend scope contract and show the normal empty result state; do not invent client-side counts or OR semantics.

### 5. Vanilla TypeScript/CSS implementation boundary

Allowed:

- Add Library-local query/scope state beside the existing `libraryView`, `libraryScope`, and `librarySelectedTagIds` state.
- Extend the existing Tauri invoke contract only through the repository's established command boundary; the design plan does not authorize Rust, DB, migration, or schema edits.
- Add render functions/classes that follow `renderLibraryNavigation`, `renderLibraryFacets`, `renderLibrary`, and the existing delegated event handlers.
- Reuse CSS variables and effective Library selectors from the end of `app/src/styles.css`; keep spacing, typography, selection, and borders aligned with `DESIGN.md`.
- Keep the dropdown positioned relative to the Search box and within the topbar stacking context.

Forbidden:

- React, Tailwind, Radix, Base UI, React Aria, `motion/react`, other UI frameworks, new dependencies, or a second token system.
- Modifying Rust, DB, migrations, canonical Paper schema, v0.2.0/v0.1.4, release/tag metadata, or unrelated Discovery UI.
- Adding a second persistent Search field, a full-screen command palette, decorative gradients/glows, or a new card-based Library layout.

### 6. Acceptance evidence for the executing agent

- At 1440/1512/1536px, the unified toolbar still aligns to the 168px sidebar and the table/Inspector split remains within the existing measured bounds.
- Search field is visible once in Library and no persistent duplicate appears in Sidebar/table/Inspector.
- Search query filters the current Library scope without changing Collection/Tag membership or the selected Inspector paper unexpectedly.
- Scope changes are visible, reversible, and retain the existing facet-pill summary.
- Collections remain nested; Tags remain flat; zero-count Tags remain visible and dimmed in both Sidebar and matching suggestions.
- Selected rows keep neutral gray selection; blue remains limited to active/focus/link/drag states.
- Long titles and metadata truncate according to the existing dense table rules; counts/years use tabular numerals.
- Empty results provide one clear recovery action and the dropdown closes with Escape/outside click.
- Existing `npx --no-install tsc --noEmit`, `npm run build`, and `git diff --check` remain green after implementation.

## Handoff summary

- `DESIGN.md`: persistent design contract, updated in RC2 with Search tokens, single-mode semantics, motion policy, and the future Inspector tab contract.
- `DESIGN PLAN`: this file, `design-plans/library-search-v021.md`.
- `SIDEBAR FINDINGS`: 168px continuous rail and recursive Collections are sound; zero-count Tag visibility needs the explicit all-tags join/dim treatment.
- `SEARCH UI FINDINGS`: the toolbar-owned Search box is implemented; RC2 removes mode switching and keeps light Collection/Tag/Paper suggestions plus one search action.
- `TOOLBAR FINDINGS`: the single 54px unified toolbar is the correct owner; do not split Search into Sidebar/table.
- `TYPOGRAPHY FINDINGS`: system UI for chrome/table, serif for Inspector title/abstract, compact 12px/11px table lanes, and tabular counts/years.
- `READY FOR UI HANDOFF`: yes; RC2 implementation and contract are aligned.

## RC2 design-engineering addendum

### Audited runtime surface

The RC2 pass re-read `app/index.html`, `app/src/main.ts`, `app/src/librarySearch.ts`, and the effective tail of `app/src/styles.css` at baseline `8eacbffc5bb12b83fc54a2ccc8b2da3602c6ba97`. The app is Vanilla TypeScript/CSS in a Tauri shell. Search input, IME composition, keyboard navigation, result filtering, and Inspector rendering are all high-frequency or direct-response paths.

| Before | After | Why |
| --- | --- | --- |
| `quick \| metadata \| content` mode selector in the toolbar | One `all` mode covering bibliographic metadata, tags, notes, and abstracts | A single search has one predictable meaning; scope stays in the existing Sidebar/facet contract. |
| Paper rows plus separate content/metadata actions in the dropdown | Matching Collections, Tags, Papers, and one `在当前范围搜索“…”` action | Paper selection locates the canonical row; the dropdown remains a lightweight scope aid instead of becoming a command palette. |
| Hard-coded Search Box/dropdown dimensions in the final CSS overrides | `--library-search-*` and `--library-toolbar-inline-gap` tokens | Search geometry can be tuned without creating a parallel component style system. |
| `libraryTagFacets.map(...)` for Sidebar Tags | `libraryTags` joined with facet counts; zero-count rows use a muted treatment | Tags are stable navigation objects; filtering must not make the hierarchy jump. |
| Any implied transition for input, keyboard navigation, dropdown, or result replacement | Explicitly static Search Box/dropdown and instant result/Inspector updates | These actions are frequent and keyboard-driven; motion would add latency without explaining a spatial change. |

### RC2 design conclusions

- Search owns one persistent toolbar control with a 430px maximum width, 250px minimum at wide layouts, 28px height, 7px inline padding, 6px radius, and a 10px inline gap from neighboring toolbar content. At narrow widths it compresses with the existing 168px/138px sidebar breakpoints without creating horizontal overflow.
- Search query semantics are all-field and local to the current Library view/scope. Collection membership remains recursive/OR, Library Tags remain flat/AND, and canonical Paper identity remains backend-owned.
- The suggestion dropdown is anchored to the Search Box, capped at three groups, uses the Search Box spacing/radius/padding tokens, keeps zero-count Tags visible and dimmed, and has no enter/exit animation. It closes on `Escape` or outside click without stealing focus.
- `⌘F`, IME composition, arrow navigation, `Enter`, `Escape`, query/result replacement, scope changes, and Inspector updates stay instant. No Search path uses `transition: all`, layout-property animation, keyframes, or an animation dependency.

### Inspector contract for v0.3.0

The current RC2 Inspector exposes only the populated `元数据` tab. The future contract is a two-tab shell with stable tab order and labels: `元数据` first, `标注` second. The `标注` tab is reserved for v0.3.0 paper-linked annotation records and future extraction workflows; it is not present in the v0.2.1 product UI. This addendum does not authorize annotation extraction, an annotation data model, persistence changes, migration, or an empty placeholder tab.

### Verification record

- Mechanical checks: `npm run test:search`, `npx --no-install tsc --noEmit`, `npm run build`, and `git diff --check` are the RC2 acceptance commands.
- Visual/interaction checks: confirm one Search Box in the Library toolbar, no mode selector, no duplicate Search field, stable 168px rail/54px toolbar alignment, nested Collections, flat dimmed zero-count Tags, neutral selected rows, instant keyboard/result updates, and no empty `标注` tab.
