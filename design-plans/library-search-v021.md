# CowPaper v0.2.1 Library Search + UI Design System

Status: ready for UI handoff. This is a design-only plan; the executing agent owns implementation and verification.

## Source and method

- Audited surface: the existing CowPaper Library workspace at `app/index.html`, `app/src/main.ts`, and `app/src/styles.css`.
- Baseline: `origin/main` / `49ea75fd3002a080ebb3c0e67acf23d11655a7b5` (`v0.2.0`).
- UI Skills source: `https://github.com/ibelick/ui-skills`.
- UI Skills source commit: `9f140de767e6e2d4adc3970eb68d24b3ec896f99` (main; commit page exposed the full SHA `9f140de`).
- UI Skills fetched/read outside CowPaper through browser retrieval of the public GitHub repository, raw `README.md`, raw `skills/*/SKILL.md`, raw `LICENSE`, and `https://www.ui-skills.com` / `/playbook`; no repository, scripts, or skill files were vendored or executed.
- `UI_SKILLS_SOURCE`: `https://github.com/ibelick/ui-skills` and `https://www.ui-skills.com`.
- `UI_SKILLS_SOURCE_COMMIT`: `9f140de767e6e2d4adc3970eb68d24b3ec896f99`.
- `IMPROVE_UI_USED`: yes; used its read-only surface trace, contract/runtime/correction proof gate, three-finding limit, and self-contained handoff structure.
- `CREATE_DESIGN_MD_USED`: yes; created the root design contract from repository tokens, rendered QA evidence, and final effective CSS rules.
- `BASELINE_UI_REVIEWED`: yes; applied only the stack-neutral guidance below.

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
| 2 | Scope is split across Sidebar rows and toolbar facet pills without a single visible query/scope model. | Sidebar owns standard views, recursive Collections, and flat Tags; `renderLibraryFacets()` only renders active scope pills in the toolbar. The user requirement explicitly calls for Search scopes and Collection/Tag scope. | Make the Search box expose a lightweight scope affordance for `All Library`, current Collection, current Tag(s), and current built-in view while preserving the Sidebar as navigation. | Library toolbar, scope state/rendering, suggestion dropdown | High |
| 3 | Zero-count Tags are not guaranteed to remain visible, so hierarchy/selection can jump after filtering. | `renderLibraryNavigation()` renders `libraryTagFacets.map(...)`; facet data is count-driven, while the requested v0.2.1 behavior requires zero-count Tags visible/dimmed. | Render all Library Tags from `libraryTags`, join counts when present, keep zero-count rows visible with muted text/dot and disabled-looking count, while leaving them selectable for future scope changes only if the product state supports it. | Sidebar Tag renderer and its count presentation | High |

## Improve first

Implement the single toolbar Search box and its scope model first. It establishes the interaction owner for all later filtering, keeps the current three-pane hierarchy intact, and avoids the highest-risk regression: two competing search fields with different semantics.

## v0.2.1 implementation plan

### 1. One Search Box

Add a single compact search control inside `.topbar-leading`, next to the current Library title/facet region. It should fit between the title/scope pills and the right-side status/actions at wide widths, and collapse to an icon/short field only under the existing responsive constraints. Do not place a persistent search field in the Sidebar, table header, or Inspector.

Recommended UI contract:

- Placeholder: `搜索文献…`.
- Search icon is a neutral supporting glyph; the clear affordance appears only when the query is non-empty.
- `⌘F` focuses the same control; `Escape` clears the query when the box is focused and closes the suggestion list when it is open.
- Query updates are local and immediate for UI feedback; debounce only if the executing agent proves the backend list command needs it.
- The query should search the Library result set across effective English title, Chinese title, authors, journal/source, note, DOI, and URL. Do not search Discovery or mutate canonical Paper identity.
- Preserve the existing `libraryView`, Collection scope, Tag scope, selected row, column widths, and Inspector width across query changes.

### 2. Search scopes

Use an explicit but quiet scope control inside the Search box, not a second toolbar control. The default is `All Library`. Available scopes:

1. `All Library`: search all papers in the Library.
2. `Current view`: search within All / Recent / Unfiled when one of those built-in views is active.
3. `Current Collection`: search within the selected Collection, including the existing parent/child scope semantics supplied by the backend.
4. `Current Tag(s)`: search within the active Tag selection. Preserve the current AND semantics for multiple Tags; do not reimplement Collection+Tag filtering in the browser.

When a Collection or Tag is active, show it as the current scope in the Search box or its anchored suggestion header. The existing `renderLibraryFacets()` pills remain the removable summary of active filters; they are not replaced by the dropdown.

### 3. Light suggestion/dropdown

The dropdown is a small anchored surface below the Search box, not a command palette. It should contain at most three groups:

- `范围`: the current scope and other available scopes with counts where available.
- `文集`: matching Collection names, with folder icon and count.
- `标签`: matching Tag names, with dot and count; zero-count Tags are still listed but muted/dimmed.

Behavior:

- Empty query + focus: show only the current scope and a short list of recently used/available scopes if local state exists; do not dump every paper.
- Non-empty query: show matching scopes/Collection/Tag names first, then an optional small `在当前范围搜索“…”` action. Selecting that action commits the query without changing the scope.
- No matches: show `没有匹配的范围或标签` and one clear `清除搜索`/`返回全部文献` action, matching the baseline empty-state rule.
- Close on outside click or Escape; preserve focus ring and do not animate the dropdown.
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
- Search field is visible once in Library and no `.library-search` duplicate appears in Sidebar/table/Inspector.
- Search query filters the current Library scope without changing Collection/Tag membership or the selected Inspector paper unexpectedly.
- Scope changes are visible, reversible, and retain the existing facet-pill summary.
- Collections remain nested; Tags remain flat; zero-count Tags remain visible and dimmed in both Sidebar and matching suggestions.
- Selected rows keep neutral gray selection; blue remains limited to active/focus/link/drag states.
- Long titles and metadata truncate according to the existing dense table rules; counts/years use tabular numerals.
- Empty results provide one clear recovery action and the dropdown closes with Escape/outside click.
- Existing `npx --no-install tsc --noEmit`, `npm run build`, and `git diff --check` remain green after implementation.

## Handoff summary

- `DESIGN.md`: created at repository root as the persistent design contract.
- `DESIGN PLAN`: this file, `design-plans/library-search-v021.md`.
- `SIDEBAR FINDINGS`: 168px continuous rail and recursive Collections are sound; zero-count Tag visibility needs the explicit all-tags join/dim treatment.
- `SEARCH UI FINDINGS`: no implemented Library Search exists; add one toolbar-owned Search box with light scope suggestions.
- `TOOLBAR FINDINGS`: the single 54px unified toolbar is the correct owner; do not split Search into Sidebar/table.
- `TYPOGRAPHY FINDINGS`: system UI for chrome/table, serif for Inspector title/abstract, compact 12px/11px table lanes, and tabular counts/years.
- `READY FOR UI HANDOFF`: yes.
