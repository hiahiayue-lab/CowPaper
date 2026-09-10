# CowPaper v0.2.2 — Full-App Design System Audit and Implementation Handoff

Status: **READY FOR IMPLEMENTATION**
Audit baseline: `origin/main = 33652b059fc20427da105976afd8ddabab423403` (`v0.2.1`)
Product version: **keep `0.2.1` until the final control stage**
Database: **v19**
Migration: **NONE**
Scope: read-only product audit plus design-document alignment. No application source, Rust, dependency, schema, release, or migration work is authorized by this handoff.

## Executive handoff

CowPaper already has a credible macOS-like Library baseline: a continuous quiet rail, a 54px workspace row, compact table rows, neutral row selection, a continuous Inspector, and static high-frequency Search interactions. The v0.2.2 work should refine ownership and consistency across the whole app rather than introduce a new visual language.

The three highest-leverage corrections are:

1. Resolve the Library Search contract drift between the current accepted implementation/tests and the older written rule. The contract is now aligned in `DESIGN.md`: one `all` mode, optional field-intent tokens, explicit suggestion selection as the commit boundary, inert Enter, and no generic Search Action.
2. Make the semantic token layer a real single owner across Discovery, Library, Settings, Activity, menus, dialogs, and Inspector. The current effective Library result is good, but the stylesheet still contains historical competing shell values and many component-local literals.
3. Give feedback, dialogs, inline editing, and background work one spatial ownership model: persistent work state in the toolbar/Activity, local validation beside the owning control, short command confirmation in the toast, and predictable focus return after popovers/dialogs.

The visual target is quiet, dense desktop software: system UI for navigation and metadata, serif only for bibliographic reading content, flat persistent surfaces, one restrained blue accent, neutral Library selection, small radii, and no decorative motion on high-frequency paths.

## Baseline and evidence

### Repository and runtime

- `git rev-parse origin/main` resolves to `33652b059fc20427da105976afd8ddabab423403`.
- `app/package.json`, `app/package-lock.json`, `app/src-tauri/Cargo.toml`, and `app/src-tauri/tauri.conf.json` remain `0.2.1`.
- `app/src-tauri/src/db.rs:101-109,4921-4955` defines `SCHEMA_VERSION = 19` and the v19 migration list. This audit does not authorize another migration.
- The product is a Tauri 2 desktop shell with Vanilla TypeScript DOM rendering. There is no React, Tailwind, Radix, Base UI, React Aria, Motion, or other UI framework in the app dependency graph.
- Primary owners are `app/index.html` for the shell, `app/src/main.ts` for state/render/event ownership, `app/src/librarySearch.ts` for framework-free Search semantics, and `app/src/styles.css` for the visual cascade.

### Read evidence

| Evidence | What it establishes | Limit |
| --- | --- | --- |
| `DESIGN.md` | Existing normative colors, type scale, geometry, state, accessibility, and motion contract | Contract, not proof that every current selector consumes it |
| `design-plans/full-app-visual-refactor-v021.md` | Prior full-app audit, accepted Library geometry, state ownership direction, and implementation boundaries | Historical v0.2.1 handoff; current source was re-opened before carrying forward any rule |
| `design-plans/library-search-v021.md` | Prior Search scope, Collection/Tag semantics, zero-count Tag behavior, and responsive acceptance evidence | Historical Search plan; current source/tests now supersede its Enter/Search Action wording |
| `docs/qa/ui-rc3/library-1512x982.png` | Accepted Library visual direction: narrow rail, compact rows, neutral selected row, thin separators, serif Inspector title | Library-only and from an earlier fixture; not a full-app screenshot |
| `docs/qa/ui-rc3/layout.json` | 168px rail, 300px Inspector, no list/document overflow at 1440/1512/1536, collapse behavior at 1000/740/600 | Synthetic 27-record fixture |
| `docs/qa/ui-rc3/interactions.json` | Existing Library interaction evidence for attachments, relations, columns, and narrow Inspector | Fixture adapters, not production Rust/SQLite execution |
| `docs/qa/library-search-v021/FULL_APP_VISUAL_CONSISTENCY_MATRIX_RC3.md` | Full-app capture matrix and responsive/state gates | Candidate QA matrix; it does not claim the current baseline passes every row |
| `app/index.html:12-325` | Shared shell, Discovery, Library, Journals, Tags, Settings, Activity, and dialog DOM | Structure and labels, not rendered pixels |
| `app/src/main.ts:628-634,1681-1809,2142-2430,2449-2480,3056-3312,3397-3468,3876-3941` | Status, cards, Search, Library/Inspector, Activity, Work Center, and modal runtime paths | Needs later implementation QA after design refinement |
| `app/src/styles.css:1-12,220-459,589-709,769-970,972-1320,1323-1696,1710-2147` | Historical style owners, current effective Library cascade, Settings, Search, focus, and reduced-motion rules | Last matching selector wins; the cascade itself is a drift risk |

The supplied visual evidence materially validates Library only. Discovery, Journals, Research Interests, Settings, Activity, dialogs, and the full state matrix are source/DOM audits in this handoff and must receive implementation captures later.

## Review method and external references

The audit used the requested outside guidance without vendoring or adding dependencies:

- [ibelick/ui-skills — improve-ui](https://raw.githubusercontent.com/ibelick/ui-skills/main/skills/improve-ui/SKILL.md): trace the rendered path, prove Contract/Runtime/Correction, keep at most three root findings, and write a self-contained plan.
- [ibelick/ui-skills — baseline-ui](https://raw.githubusercontent.com/ibelick/ui-skills/main/skills/baseline-ui/SKILL.md): apply only stack-neutral guidance; Tailwind/React-specific rules are explicitly non-applicable here.
- [ibelick/ui-skills — create-design-md](https://raw.githubusercontent.com/ibelick/ui-skills/main/skills/create-design-md/SKILL.md): keep normative values in the existing DESIGN.md frontmatter and avoid inventing a competing token schema.
- [emilkowalski/skills — emil-design-eng](https://raw.githubusercontent.com/emilkowalski/skills/main/skills/emil-design-eng/SKILL.md): frequency-first motion decisions, exact-property transitions, and a Before/After/Why review table.
- [emilkowalski/skills — apple-design](https://raw.githubusercontent.com/emilkowalski/skills/main/skills/apple-design/SKILL.md): response, predictability, spatial ownership, and reduced-motion restraint; no spring is appropriate for CowPaper's table/Search paths.
- [emilkowalski/skills — review-animations](https://raw.githubusercontent.com/emilkowalski/skills/main/skills/review-animations/SKILL.md) and [improve-animations](https://raw.githubusercontent.com/emilkowalski/skills/main/skills/improve-animations/SKILL.md): survey motion, delete unjustified/high-frequency motion, keep layout changes out of transitions, and require reduced-motion behavior.

## Design language

- **Audited product:** CowPaper desktop literature workspace, including Discovery, Library, Journals, Research Interests/Library Tags, Settings, Activity, dialogs, popovers, inline editing, and attachment flows.
- **Design direction:** macOS-native in restraint and response, not by imitating OS controls. Preserve the OS-owned title-bar/traffic-light area; use quiet surfaces, compact density, thin separators, and direct feedback.
- **Persistent surfaces:** app canvas, main content, Sidebar, and Inspector use semantic neutral surfaces. Persistent elevation is none; transient surfaces may use the one documented shadow.
- **Typography:** system UI for chrome, navigation, controls, table, metadata, Settings, and state; serif only for Inspector bibliographic title and abstract; mono only for code/path/template-like values.
- **Accent:** blue is reserved for active navigation, links, focus, explicit primary actions, and local drag feedback. Library row selection stays neutral gray.
- **Governing owners:** `DESIGN.md` owns semantic roles and geometry; `.topbar`, `.workspace-nav`, `.library-table-head/.library-paper-row`, `.library-inspector/.inspector-group`, `.settings`, and `.modal-*` own their respective compositions; `main.ts` render functions own content/state order.
- **Explicit exceptions:** Library table density and Inspector reading typography are intentional exceptions to the more spacious Discovery card surfaces. The current two-level Collection rendering guard is an implementation constraint and must be made explicit in the implementation plan; it is not permission to add a migration.

## Root findings

Only three system-level findings survived the Contract/Runtime/Correction proof gate.

| # | Problem | Contract / Runtime / Correction evidence | Proposed change | Scope | Confidence |
| --- | --- | --- | --- | --- | --- |
| 1 | Library Search prose and current accepted behavior had diverged | **Contract:** prior `DESIGN.md` said “commit on Enter or suggestion selection” and the prior Search plan included a generic Search Action. **Runtime:** `librarySearch.ts:428-467` deliberately emits field-intent suggestions and no Search Action; `main.ts:2154-2166` renders `文集/标签/搜索字段/论文`; `main.ts:2287-2290` makes Enter inert; `librarySearch.rc3.test.ts:97-145` locks those semantics. **Correction:** the current behavior is deterministic and avoids accidental highlighted-suggestion selection, so the contract should describe it rather than ask implementation to reverse it. | Update only the Search contract in `DESIGN.md` to make `all` the sole mode, field-intent tokens optional constraints, explicit suggestion selection the commit boundary, Enter inert, and generic Search Action absent. Keep the UI and adapter framework-free. | `DESIGN.md`, future Search QA copy/acceptance | High |
| 2 | The app has effective visual convergence in Library but not one durable token owner for the whole product | **Contract:** `DESIGN.md` defines a closed type scale, semantic surfaces, 168px/54px geometry, shared radii, and low decoration. **Runtime:** `styles.css:42-44` starts at a 190px rail, `styles.css:972-1189` introduces 224px/216px-era shell values, `styles.css:1323-1696` adds RC3/RC5 overrides, and `styles.css:1710-2147` adds a later DS layer. The final Library result is correct, but many Discovery/Settings/modal values remain literal and component-local. **Correction:** future changes cannot reliably infer the governing value from one owner, which is a visual-drift risk across workspaces. | Consolidate one semantic token block and component-role layer, then migrate by surface. Do not mechanically rewrite the whole stylesheet in one pass. Preserve the accepted Library computed geometry while making Discovery/Settings consume the same roles. | `app/src/styles.css`; no DOM or backend change required for the design handoff | High |
| 3 | Feedback and focus ownership is still split across toolbar, toast, local fields, and dialogs | **Contract:** persistent background work belongs to toolbar → Activity; validation belongs to its field/region; toast confirms a short command; dialogs restore focus to their invoker. **Runtime:** `main.ts:628-634` sends many operations through one bottom `#status` toast while `main.ts:3397-3468` separately renders `#work-status`; background PDF events also call `setStatus` at `main.ts:4264-4278`; `main.ts:3876-3917` focuses Cancel but does not capture/restore the invoker or contain Tab. **Correction:** the same event can compete for attention, and closing a dialog loses keyboard context even though the current global `:focus-visible` ring exists at `styles.css:1852-1860`. | Implement a state-ownership matrix and focus-continuity pass: persistent work status stays in toolbar/Activity, short command confirmation stays in the toast, field/region errors stay local, dialogs capture invoker/focus entry/Tab/Escape/overlay/return, and popovers close back to their trigger. Keep motion static for high-frequency paths. | `app/src/main.ts`, `app/src/styles.css`, `app/index.html` dialog attributes only; no backend change | High |

### Already resolved and not to regress

- The all-field searchable set and excluded metadata are explicit in `DESIGN.md` and `librarySearch.ts:11-51`.
- Zero-count Tags are joined from the complete `libraryTags` list in `main.ts:2337-2346`; they must remain visible and dimmed.
- The accepted Library table uses a shared grid, 27px header, 30/39px rows, neutral selection, and no page-level overflow.
- The effective CSS already includes a global focus ring at `styles.css:1852-1860`, static Search transitions, and a reduced-motion media query at `styles.css:2133-2147`. Do not reopen the stale v0.2.1 finding that these rules are entirely absent; implementation QA should verify coverage and regressions instead.
- The Inspector remains metadata-only in v0.2.1. Do not add an empty Annotation tab, extraction control, schema, or migration.

## Limited token contract

This is the v0.2.2 implementation contract. It reuses the existing `DESIGN.md` values and deliberately stays small. Do not create feature-specific token families.

### Typography

| Role | Value | Use |
| --- | --- | --- |
| `ui` | system stack, 14px / 20px / 400 | default controls and app chrome |
| `caption` | 10px / 14px / 500 | section labels, compact headings, status detail |
| `meta` | 11px / 16px / 400 | table metadata, helper text, counts where compact |
| `body-compact` | 12px / 16px / 400 | dense Library and small UI copy |
| `body` | 13px / 20px / 400 | Discovery card body and ordinary form copy |
| `section` | 14px / 20px / 650 | section headings and compact navigation emphasis |
| `paper-title` | 15px / 21px / 650 | Discovery paper cards; Library title stays lower-emphasis at its documented table size |
| `title` | 16px / 22px / 600 | view headings and Settings group headings |
| `display` | 18px / 22px / 650 | rare page-level emphasis only; not a Library row style |
| `reading` | Georgia, 12px / 20px / 400 | Inspector abstract |
| `inspector-title` | Georgia, 16px / 1.35 / 700 | selected paper title in Inspector |
| `mono` | ui-monospace, 11px / 16px / 400 | file paths, PDF template, technical identifiers when necessary |

Rules: system UI owns navigation, buttons, filters, table, labels, Settings, and state. Serif is limited to bibliographic reading content. Counts, years, progress, and scores use tabular numerals. Do not add a new size without recording a component exception.

### Spacing and geometry

Use only the existing rhythm: `1, 2, 4, 6, 8, 10, 12, 14, 16, 18, 20, 24, 28px`. Component geometry is explicit: 168px desktop rail, 138px narrow rail, 54px workspace row, 34px navigation row, 27px table header, 30px normal table row, 39px row with secondary title line, 28px Search control, 300px Inspector baseline with a 300–560px resize range, and 680px Settings content maximum.

Use 6px control gap/padding, 8px compact grouping, 10–12px inline/table gaps, 16px content inset, and 24px section separation. Keep the table header and rows on one shared grid. Do not add a second content inset to the Library panes.

### Radius, borders, surfaces, and selection

| Role | Value / rule |
| --- | --- |
| `none` | 0px; continuous surfaces and separators |
| `micro` | 4px; inline controls and tiny tokens |
| `row` | 5px; dense Library rows and menu items |
| `control` | 6px; buttons, inputs, Search, compact controls |
| `card` | 10px; Discovery cards and grouped Settings surfaces |
| `group` | 12px; larger Settings or relation groups when a boundary is needed |
| `modal` | 16px; dialogs only |
| `pill` | 999px; semantic chips only, not ordinary buttons |

Persistent surfaces use borders and whitespace with no shadow. Transient menus/popovers/drop queues/dialogs may use `0 8px 24px #00000014`. Do not add gradients, glow, glass blur, or decorative elevation. The current modal overlay's broad `backdrop-filter: blur(4px)` at `styles.css:597-598` should be removed or reduced to a simple dim layer during implementation so the dialog remains the only focal surface.

Surface roles: app `#f5f6f8`, main `#ffffff`, Sidebar `#f6f6f6`, Inspector `#f5f5f5`, subtle `#f4f6f8`, control `#f4f7fa`, hover `#f2f2f2`, active `#ececec`, selection `#dedede`, selection-hover `#d8d8d8`, drop `#eef5fd`, primary text `#303030`, secondary `#898989`, muted/placeholder `#aaaaaa`, default border `#e3e6ea`, divider `#e9e9e9`, control border `#dedede`, accent `#287cff`, focus `#3487ff`.

Selection contract: Library row selection is neutral gray; blue is for active navigation, links, focus, explicit primary actions, and local drag feedback. Tag dots, state colors, and opacity are supplemental and never the only state carrier.

### Motion

| Frequency / surface | Contract |
| --- | --- |
| Search typing, IME, keyboard navigation, filtering, result replacement, row selection, Inspector refresh | no animation |
| Menus/popovers/dialogs when spatial feedback materially helps | transform + opacity only, 125–250ms ease-out, trigger-aware origin; no layout properties |
| PDF queue progress | instant update or transform-based `scaleX`; never transition `width` |
| Reduced motion | remove movement and looping; retain state, focus, text, and progress information |

Before adding any motion, record its purpose and frequency. Never use `transition: all`, `ease-in`, keyframe restarts on rapid input, or animation on a keyboard action. The existing Search `animation: none`/`transition: none` rules are intentional.

## Surface-by-surface audit and handoff

### Window chrome and workspace switcher

Current owner: `index.html:12-67`, `.app/.sidebar/.topbar`, `main.ts:3672-3694`, and the final CSS layers.

- Preserve the OS-owned title-bar/traffic-light space. Do not draw fake traffic lights or move the native window controls into HTML.
- Keep one 54px workspace row across Discovery and Library. The workspace tabs sit in the continuous rail; the main toolbar begins at the 168px rail edge in Library.
- The current cascade contains old 46px, 48px, and 54px declarations. Future implementation must make the 54px value the single effective owner and verify mode switching at 1440px and 600px.
- Discovery must not reserve a disabled Library Search slot. Library must show exactly one Search control.

### Discovery, recommendations, history, All Papers, and Favorites

Current owners: `index.html:69-118`, `main.ts:1582-1809`, and styles `220-341,461-585,767-970` plus the DS layer.

- Keep the order title → abstract/summary → metadata/status → next action in paper cards. Optional Chinese title, summary, score, tags, and missing/partial notices must not cause action placement to jump.
- Keep filters as compact metadata controls; they are not another global Search surface.
- Recommendation score is supporting metadata. AI tag chips may be stronger than Collection badges; Collection badges are low-emphasis and never encode score.
- Empty Today/History/Favorites/All Papers states retain their heading/context and expose one useful next action where possible. Do not add illustrations, gradients, or skeletons by default.
- Use system UI and the shared 15px paper-title / 13px body / 12px metadata hierarchy. Cards may use the 10px card radius; do not carry Library row density into Discovery cards.

### Toolbar and Sidebar

- Toolbar actions use 30px effective control height, 6px radius, quiet ghost treatment, and visible focus. The blue primary treatment is reserved for the one next action in a local flow.
- Icon-only controls in `index.html:62-64` keep accessible names and gain a consistent pressed/expanded state where a menu or Inspector is open. Glyphs remain secondary; do not add icon containers or ornamental badges.
- Sidebar rows use 34px height, 6px radius, 24px section gap, and right-aligned tabular counts. Management affordances stay quiet until hover or keyboard focus.
- The current `renderLibraryNavigation()` guard at `main.ts:2320-2321` intentionally hides depth greater than one. Because the database/backend contract remains frozen, the implementation should either explicitly document the two-level limit in `DESIGN.md` or provide a product decision before rendering deeper trees; do not silently claim unlimited recursion.

### Library Search

Current owners: `index.html:41-47`, `main.ts:2044-2334`, `librarySearch.ts:11-167,428-467,596-735`, and styles `1668-1730,2000-2074`.

- One 430px maximum / 250px minimum Search control at 28px height, with compact tokens and a light anchored popover. At 900px and 620px breakpoints it compresses without a second row or page overflow.
- `all` is the only mode. Free text searches the nine allowed fields. Optional field-intent suggestions narrow the all-field query; they are not a mode selector.
- Suggestion groups are `文集`, `标签`, `搜索字段`, and `论文`. There is no generic `Search Action` row. Empty-query focus remains quiet. Selecting a suggestion is explicit; Enter is inert; Escape closes and then clears on the next press.
- Zero-count Tags stay visible and dimmed. Collection hierarchy stays structural; Tag hierarchy stays flat.
- The existing `library-match-evidence` markup at `main.ts:2274-2282,2399` has no dedicated CSS owner. If search-hit fields/snippets remain visible, give them a compact, non-row-expanding treatment inside the title cell; otherwise remove the markup. Do not let evidence silently change the 30/39px table baseline.
- Never animate typing, IME, suggestion open/close, keyboard navigation, filtering, selection, or Inspector updates.

### Paper Table and attachment children

Current owners: `index.html:120-130`, `main.ts:2367-2430`, `main.ts:2456-2480,2680-3050`, styles `1355-1498,2087-2091,2204-2270`.

- Keep the shared grid for header and rows, 27px header, 30px normal row, 39px row with Chinese title, 12px column gap, and neutral selected row.
- Title is primary; Note, Journal/Source, Year, and Authors are secondary. Long values ellipsize with title/hover access to the full value. Years and counts use tabular numerals.
- A PDF is a child of a Paper. Child rows use subordinate typography and local separators; managed, linked, missing, processing, complete, and failed states are named in text.
- Drag feedback is local to the target row/sidebar item/list pane. Drop overlays and queue surfaces may be transient but do not become a page-wide blue state.
- The current attachment action rules intentionally keep controls in document flow in the final Library layer. Preserve that behavior so actions do not cover filenames.
- At 1440/1512/1536 keep the measured 168px rail/300px Inspector/no-overflow baseline. At 1000 the Inspector collapses; at 740 Note hides; at 600 Authors hides and the rail is 138px.

### Inspector and inline edit

Current owners: `main.ts:2449-2480`, styles `1376-1401,1862-1900,2106-2127,2275-2320`.

- Keep one continuous `#f5f5f5` reading surface. Grouping is by thin top rules and spacing, not nested cards.
- The selected paper title is Georgia 16px/1.35 bold; abstract is Georgia 12px/1.7. Labels are 10px and values 11px with a 60px label column.
- Keep only the populated `元数据` tab in v0.2.1. The future Annotation contract remains documentation-only.
- Inline edits must preserve the field's type hierarchy, show a visible focus ring, commit/cancel predictably, and return focus to the edited field or invoker. Never turn a one-field edit into a new modal unless the content genuinely requires it.
- Relation chips distinguish folder/Collection semantics from flat Tag dots. Chips may be pill-like; surrounding row controls remain 5–6px radius.
- Empty metadata uses muted text plus a nearby edit/add action. Missing PDF uses explicit copy and `重新链接`/`添加 PDF` recovery.

### Settings

Current owners: `index.html:181-290`, `main.ts:925-1048,3730-3862`, styles `1280-1320,1946-1980`.

- Keep the 680px content maximum, system UI, 30px field height, 6px control radius, 16px section heading, and grouped Update/DeepSeek/AI/Abstract/PDF/Sync order.
- Use two-column label/control rows on wide windows and stack them on narrow widths. Helper copy stays adjacent to its control.
- Keep API key, updater, PDF root, naming template, and sync validation statuses local to their owning group. A bottom toast may confirm completion but cannot be the only explanation.
- Destructive actions (`删除 Key`, reset-like actions) remain explicit and visually secondary. PDF move warnings are warning-role copy, not failure red.
- Token chips are utility tokens (mono, compact, removable only when semantically removable), not decorative tags. The `Library` badge remains low emphasis.
- Remove the broad modal blur treatment when normalizing dialogs; Settings should retain a plain desktop panel feel.

### Menus, popovers, dialogs, and inline feedback

Current owners: Work Status `index.html:49-59`, Column menu `60-64`, Search `41-47`, relation editor `main.ts:2432-2440`, context menu `main.ts:3232-3268`, dialogs `index.html:308-325` and `main.ts:3876-3941`.

- Popovers are trigger-owned, bounded, keyboard-closable, and do not steal focus. Use the transient surface role and one shadow.
- Search is static. Status/column/relation/context menus may remain static unless a later product decision explicitly authorizes a spatial transition.
- Dialogs use 16px radius and max 440px width. Use explicit destructive copy and correct button order. Capture the invoker, focus the first safe control or input, contain Tab, close on Escape/overlay, and restore focus to the invoker.
- The current modal icon and centered copy are more ornamental than the rest of the product. Keep content hierarchy, but reduce decoration to a quiet title/message/action stack unless a specific confirmation type needs an icon.
- Inline statuses use short, human-readable copy and the owning surface. Avoid duplicating the same message in bottom toast, toolbar, and page body.

### Buttons, chips, tokens, Collection tree, Library Tags

- Primary: blue fill, white text, one next action. Ghost: border/neutral or quiet text. Small: compact 11–12px utility. Icon-only: accessible label, 30px focusable target, no unlabeled glyph-only dependency.
- Hover is a subtle surface change. Focus is the visible `#3487ff` ring. Disabled state uses native `disabled` semantics plus helper copy where the prerequisite is not obvious; opacity alone is insufficient.
- AI tag chips, Collection badges, Library Tag dots, relation chips, Search scope tokens, and Settings PDF template tokens are different semantic families. Share the same spacing discipline but not a single undifferentiated hue system.
- Collections are structural and use folder symbols, indentation, additive membership, and counts. Tags are flat, use a dot plus text, and treat color as supplemental.
- Search scope tokens live inside the Search control; do not recreate a second facet/chip row beside the toolbar.

### Empty, loading, error, success, and AI status

Use one state order everywhere: explain what is absent/happening, retain context, then offer one clear next action.

| State | Owner and treatment | Example |
| --- | --- | --- |
| Empty | owning list/card/Inspector region; muted, contextual, no illustration required | `从发现页收录论文`, `清除搜索`, `添加 PDF` |
| Loading | owning region or persistent Work Status; retain heading/controls | `读取中…`, `分析中`, `导入中…` |
| Partial/missing | warning role with retained record and source explanation | `可能不完整`, `文件缺失`, `重新链接` |
| Error | adjacent to failed field/region with recovery | retry, relink, test connection, inspect Activity |
| Success | short inline confirmation or toast, resulting data remains | `已保存`, `PDF 已打开` |
| Background work | toolbar `#work-status` → Activity detail; text/count/action, not dot-only | `同步中`, `待分析 3`, `分析失败` + retry |

AI status text remains the source of truth: `等待摘要`, `待分析`, `排队中`, `正在分析`, `已分析`, `AI 分析失败`. Status color must be paired with text/action; partial abstracts remain warning amber rather than failure red.

## Motion review table

| Before | After | Why |
| --- | --- | --- |
| `transition: width .2s ease` on the PDF progress span at `styles.css:1258` (later neutralized by the DS tail) | Keep progress instant or use `transform: scaleX(...)` with a transform origin; retain the reduced-motion instant path | Width is a layout/paint property and progress does not need interpolation in a dense desktop queue |
| Broad `backdrop-filter: blur(4px)` on `.modal-overlay` at `styles.css:597-598` | Use a quiet dim overlay; reserve the documented transient shadow for `.modal-card` | Keeps the low-decoration macOS-like visual language and avoids turning every dialog into a glass surface |
| Dialog focus always lands on Cancel at `main.ts:3916` and closing only removes the overlay | Capture invoker; focus input/first safe action; contain Tab; restore invoker on all close paths | Keyboard users retain spatial continuity and do not lose context |
| Search suggestions are static and keyboard/IME paths are immediate | Preserve static behavior; do not add fade, spring, keyframe, or layout transition | High-frequency input must not acquire perceived latency |

## Implementation sequence

1. **Contract and token owner.** Keep the `DESIGN.md` Search correction already applied. Define one final semantic token block in CSS for type, spacing, radius, surfaces, borders, selection, focus, and motion policy. Do not bump version or schema.
2. **Shared chrome.** Normalize the effective 54px toolbar, 168px/138px rail, row rhythm, control height, and focus treatment across Discovery/Library/Settings. Preserve native title-bar space.
3. **State and focus ownership.** Add the state matrix and dialog/popover focus continuity. Keep persistent work status, local field feedback, and toast confirmation distinct.
4. **Discovery and Settings refinement.** Apply shared roles to cards, filters, Journals, Research Interests, Settings, and Activity without changing data order or backend behavior. Capture empty/loading/error/success states.
5. **Library preservation pass.** Re-verify Search, sidebar, Collection/Tag tree, table, attachments, Inspector, inline edit, menus, and responsive geometry against the accepted fixture. Keep current zero-count Tag behavior and metadata-only Inspector.
6. **Visual QA.** Capture 1440/1512/1536/1000/740/600 states, plus Search open/selected/no-result, status popover, column menu, relation editor, attachment child, inline edit, Settings warning/success/error, dialog open/close, reduced motion, and cross-workspace switching.

## Acceptance checklist

### Contract and scope

- [ ] `origin/main` baseline remains `33652b059fc20427da105976afd8ddabab423403` for this handoff.
- [ ] App/product metadata remains `0.2.1` until final control stage.
- [ ] DB remains v19; no migration or schema work is introduced.
- [ ] No React, Tailwind, Radix, Motion, new dependency, or second token schema is introduced.
- [ ] `DESIGN.md` and this plan are the only design-document changes unless the executor is separately authorized.

### Type, density, and geometry

- [ ] One semantic type hierarchy is used; system UI/serif/mono roles are limited as specified.
- [ ] Shared 54px toolbar, 168px desktop rail, 34px nav rows, 680px Settings width, 27px table header, and 30/39px Library rows hold at all supported widths.
- [ ] 1440/1512/1536 retain 300px Inspector, no document/list horizontal overflow, and aligned header/row columns.
- [ ] 1000/740/600 retain usable table geometry, correct column hiding, one Search control, and narrow Inspector behavior.

### Search/sidebar/table/Inspector

- [ ] Search is visible once in Library and absent from Discovery/Settings.
- [ ] `all` is the only Search mode; field-intent tokens are optional constraints, not a mode selector.
- [ ] Explicit suggestion selection commits; Enter does not auto-select; Escape closes then clears.
- [ ] Collections remain structural; Tags remain flat; zero-count Tags remain visible/dimmed.
- [ ] Search evidence does not expand a normal row beyond the 30/39px contract.
- [ ] Selected Library rows remain neutral gray; blue is reserved for active/focus/link/drag.
- [ ] Inspector remains continuous and metadata-only, with serif title/abstract and no Annotation placeholder.
- [ ] Attachments remain child rows with named state and local actions.

### Settings, states, menus, and motion

- [ ] Settings feedback is local; destructive actions are explicit and secondary.
- [ ] Work status, toast, and field/region feedback do not duplicate ownership.
- [ ] Menus/popovers close predictably and return focus to their trigger.
- [ ] Dialogs capture invoker, manage focus, close on Escape/overlay, and restore focus.
- [ ] No `transition: all`, layout-property animation, keyboard-path animation, or broad decorative blur remains.
- [ ] Reduced motion removes movement while retaining state/progress information.
- [ ] Empty/loading/partial/error/success states preserve context and provide one clear recovery action where possible.

### Verification commands for the implementation agent

Run only after implementation changes are authorized:

```text
npm run test:search
npm run test:search:rc2
npm run test:search:rc3
npx --no-install tsc --noEmit
npm run build
git diff --check
```

The visual/interaction evidence must be recorded separately from fixture/backend claims. Existing fixture captures do not prove production Rust/SQLite behavior.

## Explicit non-goals

- No version bump, release/tag change, or final sign-off.
- No Rust, SQLite, v19 FTS, migration, schema, or canonical Paper changes.
- No Annotation UI, extraction, persistence, or migration.
- No backend behavior changes hidden inside a visual refactor.
- No broad rewrite of `app/src/main.ts`, `app/src/styles.css`, or `app/index.html`; implementation should proceed by owner and preserve the accepted Library baseline.

## Handoff status

**READY FOR IMPLEMENTATION.** The design contract is finite, the only necessary document alignment is applied to `DESIGN.md`, and the remaining work is a scoped Vanilla TypeScript/CSS implementation plus visual/keyboard QA.
