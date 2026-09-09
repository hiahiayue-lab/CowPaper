# CowPaper v0.2.1 RC3 — Full-App Visual Consistency Matrix

Status: **QA preparation / candidate gates not executed**
Baseline: 5b8355d70a72a49974586e940403176cf794bf18
Visual fixture viewports: fixtures-rc3.json
Existing layout evidence: docs/qa/ui-rc3/layout.json, docs/qa/ui-rc3/README.md

This is the visual oracle for the whole app, with extra attention to the
Library Search handoff. It checks shared chrome across Discovery, Library, and
Settings, then checks Library-specific toolbar/sidebar/table/Inspector and
popover states. It is not a pixel-diff requirement: measure the anchors below,
compare against the accepted baseline evidence, and record any intentional
candidate change before calling it a regression.

## Visual source of truth

- Shared chrome is quiet, neutral, dense, and aligned to the existing grid. Blue is reserved for active, focused, linked, or drag-target states; selected Library rows remain neutral gray.
- The Library rail is 168px at desktop/tablet widths and 138px at the 600px narrow case. The desktop Inspector is 300px in the accepted 1440/1512/1536 evidence.
- Library table header and rows share the existing library column grid; the header is 27px high and rows remain compact. Title is primary; Note, Source/Journal, Year, and Authors are secondary.
- The Inspector is one continuous light-gray metadata surface with a serif paper title/abstract, thin section rules, and one visible populated 元数据 tab in v0.2.1.
- Search is one 28px toolbar control with a light anchored popover. Search popover open/close and result replacement are static/instant; no new animation is needed.

## Capture protocol

Use the same synthetic fixture database, candidate SHA, OS/runtime, and zoom
for every capture. Capture at least one clean baseline shot and one targeted
state shot for each row that names a control. Suggested filenames:

~~~text
rc3-<matrix-id>-<viewport>-<state>.png
~~~

For each capture record: candidate SHA, viewport, device-pixel ratio, active
workspace/view, visible Search query/tokens, selected Paper, and any measured
anchor. When a popover is involved, capture its trigger and the full anchored
surface in the same frame.

## Viewport anchors

| Viewport | Expected shell anchors | Required responsive checks |
|---|---|---|
| 1440×982 | Sidebar 168px; Inspector 300px; Library pane has no horizontal overflow. | Full desktop chrome, toolbar, five-column table, Inspector, Search popover, status/column/relation popovers. |
| 1512×982 | Sidebar 168px; Inspector 300px; pane width and columns expand without drift. | Compare with accepted 1512 evidence; selected row and Inspector remain aligned. |
| 1536×982 | Sidebar 168px; Inspector 300px; no document/list overflow. | Wide-toolbar spacing and long-label truncation. |
| 1000×982 | Sidebar 168px; Inspector collapsed/hidden. | Table remains usable; toolbar actions do not overlap; Search remains one control. |
| 740×982 | Sidebar 168px; Inspector collapsed; Note column hidden. | Title/Source/Year/Authors remain visible; no list/document horizontal overflow. |
| 600×982 | Sidebar 138px; Inspector collapsed; Note and Authors hidden. | Title/Source/Year remain visible; narrow Search control does not clip or create a second row. |

## Matrix

| ID | Surface/state | Visual assertion | Evidence / measurement | Type / prep status |
|---|---|---|---|---|
| V01 | Workspace switcher | Discovery and Library tabs share the same 54px rail header, baseline, type scale, and active underline. Switching workspaces does not resize the shell. | Side-by-side screenshots at 1440 and 600; rail width measurement. | Candidate gate |
| V02 | Discovery sidebar | Discovery navigation has the same rail rhythm as Library: stable padding, item height, section spacing, right-aligned counts, and one restrained active accent. | Screenshot of Today/History/Favorites/Journals/Tags/Activity; measure item bounds. | Candidate gate |
| V03 | Discovery toolbar | Discovery title, status, sync, AI, settings, and icon buttons occupy one aligned toolbar with no Library Search control visible or reserved blank slot. | 1440 and 740 screenshots; DOM/display check. | Candidate gate |
| V04 | Discovery content | Recommendation/cards/list typography, metadata hierarchy, selected/hover state, and empty/recovery state remain unchanged when Library Search has a committed query. | Before/after Discovery screenshot and request log showing no Library filtering. | Candidate gate |
| V05 | Library toolbar | One continuous toolbar begins at the 168px rail edge, keeps the 54px baseline, and fits Search plus Import/status/sync/AI/column/Inspector/settings controls without overlap. | 1440/1512/1536 screenshots; left edge and height measurements. | Candidate gate |
| V06 | Search control | Search field is 28px high with the documented radius/padding/border; focus ring is visible and local; icon, tokens, free text, clear button, and placeholder align on one line. | Focused/unfocused screenshots at 1440, 1000, 600; computed box measurements. | Candidate gate |
| V07 | Search popover | Popover is anchored directly below Search, uses the same spacing/radius/item-padding tokens, stays within the toolbar stacking context, and does not shift the table or Inspector. | Open-popover screenshots with trigger visible; compare table top edge before/after. | Candidate gate |
| V08 | Search Action removal | Non-empty Search has no 操作/Search Action row and no mode selector styling; Collection/Tag/Paper suggestions retain the same light hierarchy. | DOM snapshot plus screenshot for governance; allowed/forbidden kinds from S02. | Candidate gate; baseline action row is a known gap |
| V09 | Library sidebar | Library views, nested Collections, and flat Tags retain stable indentation, row height, count alignment, and active/hover treatment while Search scope changes. | Screenshots with AI → AI/Governance, ESG, Core, Review; measure depth increments. | Candidate gate |
| V10 | Zero-count Tags | Zero-count Library Tags remain visible in the rail and Search suggestions, with muted label/dot/count but readable name, clickable affordance, and valid drag target. | Fixture tag Zero Count; screenshot plus DOM dimmed/draggable attributes and drag recording. | Candidate gate |
| V11 | Library table header | Header and rows use the same column grid, thin separator, compact height, low-contrast labels, and tabular Year/count alignment. | Overlay or measured screenshot at 1440/1512; compare docs/qa/ui-rc3/layout.json column widths. | Candidate gate |
| V12 | Library table rows | Title is primary; source, note, year, and authors are secondary; long values ellipsize without horizontal overflow; Chinese title remains a muted second line. | Fixture A–M long/CJK rows at desktop and narrow widths; cell bounding boxes. | Candidate gate |
| V13 | Row selection/hover | Selected rows use neutral gray, not a second blue highlight; hover/focus feedback is local and does not change row height or column alignment. | Screenshot of selected Paper 101 with Search open/closed; keyboard focus outline. | Candidate gate |
| V14 | Inspector | Inspector remains a continuous light-gray reading surface with serif title/abstract, thin section rules, stable labels/values, and no Search-specific duplicate metadata panel. | Selected Paper 101 screenshots before/after query; compare group positions. | Candidate gate |
| V15 | Inspector resize/collapse | Desktop resizer is visible and keyboard-focusable; collapse/reopen preserves table width and selected Paper. Narrow widths use the existing collapsed/overlay behavior without clipping Search. | 1440 resize recording; 1000/740/600 collapsed and reopened screenshots. | Existing layout evidence + candidate gate |
| V16 | Inspector tabs / Annotation boundary | RC3 shows the populated 元数据 tab only. No empty 标注 tab, annotation placeholder, extraction control, or annotation badge appears. | Inspector DOM/screenshot with Paper selected; compare future contract fixture. | Candidate gate / future contract guard |
| V17 | Status popover | Work-status trigger, dot, label, popover, detail text, and 查看活动 action share the toolbar’s compact rhythm; popover is locally anchored and closes cleanly. | Open/closed screenshots at desktop/narrow widths; anchor distance measurement. | Candidate gate |
| V18 | Column menu | Column-management icon and menu align to the toolbar; menu has clear title, compact checkbox rows, reset action, focus/disabled states, and no layout shift when opened. | Open-menu screenshot; toggle one non-title column and compare table baseline. | Candidate gate |
| V19 | Relation popover | Collection/Tag relation picker in the Inspector uses the same light, compact popover language as Search without looking like a second Search surface; selected chips preserve the established folder/dot treatments. | Inspector relation picker screenshot with Collection and Tag chips. | Candidate gate |
| V20 | Buttons | Primary, ghost, small, icon, danger, disabled, hover, and keyboard-focus states use the existing color/weight/radius language. No new gradient/glow or oversized control appears. | State sheet for toolbar, Search clear, popover, table/Inspector actions, and Settings save. | Candidate gate |
| V21 | Tags and chips | Library Tag dots, Search scope tokens, facet pills, relation chips, and Settings PDF token chips remain visually distinct but share spacing, truncation, and removal affordance discipline. No external/provider/duplicate Search chip appears. | Screenshot set for Core/Review/Zero Count plus PDF template tokens; DOM token count. | Candidate gate |
| V22 | Empty states | Empty Library, no-match Search, empty Inspector, empty Discovery list, and empty Activity/Settings substate have readable hierarchy and one clear recovery/action affordance where applicable. | Trigger each empty state; screenshot and action label record. | Candidate gate |
| V23 | Settings shell | Settings uses the same global toolbar/rail alignment, system UI typography, compact labels/controls, section rules, and restrained blue action state; Library Search is hidden and has no residual popover. | Settings screenshot at 1440/740/600; DOM visibility check. | Candidate gate |
| V24 | Settings controls/popovers | Settings inputs/selects, PDF root chooser/reset, template token chips/preview, warnings, and Save feedback retain alignment, wrapping, focus, disabled, and validation states. | Screenshot of normal, warning, and saved/error states without changing user data. | Candidate gate |
| V25 | Full-app overflow | At every viewport, document and app-shell horizontal overflow is false unless a deliberately scrollable local pane is documented; Search/popovers do not cause horizontal page overflow. | Browser metrics plus screenshots at all six widths. | Candidate gate |
| V26 | Typography/density/contrast | System UI is used for chrome/table/labels; serif treatment is limited to Inspector reading content; counts/years are tabular; muted text remains legible; one blue accent is preserved. | Computed-style snapshot and visual review across Discovery/Library/Settings. | Candidate gate |
| V27 | Default motion | Search focus, typing, suggestion open/close, result replacement, row selection, and Inspector refresh are instant. Any other transition is local, exact-property, and spatially meaningful. | Screen recording with rapid typing/keyboard; computed-style inspection for Search. | Candidate gate |
| V28 | Reduced motion | With OS prefers-reduced-motion: reduce, movement is removed while useful color/opacity feedback remains. No Search keyframe restart, layout-property animation, or transition: all. | Matched normal/reduced recordings and computed-style snapshot. | Candidate gate |
| V29 | Cross-workspace consistency | Switch Discovery → Library → Settings → Library at the same viewport. Shared rail/toolbar edges stay fixed; Library Search never leaks into Discovery/Settings and returning to Library does not visually reset table/Inspector unexpectedly. | Four-state storyboard at 1440 and 600; shell anchor measurements. | Candidate gate |
| V30 | Fixture result evidence | Search result rows, snippets/matched-fields indication, selection, and Inspector all keep the same canonical Paper identity and visual hierarchy after Collection/Tag/free-text filtering. | AI 治理, ESG 平台, mixed query screenshots plus raw result JSON. | Candidate gate; result payload handoff required |

## Reduced-motion and implementation guardrails

The visual review should flag any Search path that introduces transition: all,
animates layout properties, restarts keyframes on rapid input, or changes table
geometry while the popover opens. A reduced-motion setting is not evidence that
the default Search surface may animate: the default contract is also instant.

## Annotation boundary

The visual contract intentionally verifies absence. The v0.2.1/RC3 Inspector
has one populated 元数据 surface; 标注 is a future v0.3.0 direction only.
Do not add an empty tab, placeholder card, extraction affordance, annotation
storage, or migration to make V16 appear complete.

## Exit condition

The matrix is **READY FOR INDEPENDENT QA** once a candidate SHA and matching
fixture/runtime are available for capture. It does not claim that the current
baseline or any candidate has passed the rows above.
