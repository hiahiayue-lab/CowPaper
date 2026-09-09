# CowPaper v0.2.1 RC3 — Full-App Visual Refactor / Design-System Audit

Status: **READY FOR IMPLEMENTATION**  
Audit baseline: `5b8355d70a72a49974586e940403176cf794bf18`  
Scope: read-only audit plus implementation handoff. This commit changes only `DESIGN.md` and this plan.

## Boundaries and evidence

The checkout was verified at the requested SHA before the audit and was clean. The audited implementation is a Tauri 2 desktop shell with a Vanilla TypeScript DOM renderer, `app/index.html` as the surface skeleton, `app/src/main.ts` as state/render/event ownership, `app/src/librarySearch.ts` as the framework-free Library Search reducer, and `app/src/styles.css` as the visual implementation. No motion library, keyframe animation, or component framework is present.

Evidence was read from the rendered path and source owners:

| Evidence | What it proves | Limit |
| --- | --- | --- |
| `docs/qa/ui-rc3/library-1512x982.png` | Library continuous rail, table, selected row, and Inspector typography | Library only; not a full-app visual pass |
| `docs/qa/ui-rc3/layout.json` | 168px rail, 300px Inspector, no document/list horizontal overflow at 1440/1512/1536 and responsive collapse at 1000/740/600 | Synthetic 27-record fixture |
| `docs/qa/ui-rc3/interactions.json` | 21 successful Library interaction assertions, including attachments, relations, column layout, and narrow Inspector | Fixture command adapters, not Rust/SQLite verification |
| `app/index.html:1-67,69-179,181-304,308-325` | Window chrome, Discovery, Library shells, Journals, Tags, Settings, Activity, and dialog DOM | Structure, labels, and ARIA—not rendered pixels |
| `app/src/main.ts:618-624,1571-1799,2024-2388,2439-3025,3056-3312,3572-3650` | Status ownership, paper/recommendation rendering, Search, Library/Inspector, PDF flow, Activity, and modal behavior | Runtime paths require implementation QA after visual refactor |
| `app/src/styles.css:1-12,125-459,589-652,769-950,1280-1320,1323-1696` | Base tokens, Discovery controls/cards, modal, recommendation/journal/tag/settings styles, and effective RC3/RC5 Library cascade | The file contains historical overrides; the last matching rule is effective |

The provided visual fixture does not render Discovery, Settings, Journals, Tags, or Activity. Those surfaces are therefore handed off as source/DOM audits with explicit implementation acceptance criteria; they are not described as screenshot-validated.

## Skills and review method applied

The repo did not contain local copies of the requested third-party skills. Their public instructions were read and applied from outside the repo; none were vendored, added to dependencies, or committed:

- ibelick/ui-skills: [improve-ui](https://raw.githubusercontent.com/ibelick/ui-skills/main/skills/improve-ui/SKILL.md), [baseline-ui](https://raw.githubusercontent.com/ibelick/ui-skills/main/skills/baseline-ui/SKILL.md), and [create-design-md](https://raw.githubusercontent.com/ibelick/ui-skills/main/skills/create-design-md/SKILL.md).
- emilkowalski/skills: [emil-design-eng](https://raw.githubusercontent.com/emilkowalski/skills/main/skills/emil-design-eng/SKILL.md), [apple-design](https://raw.githubusercontent.com/emilkowalski/skills/main/skills/apple-design/SKILL.md), [review-animations](https://raw.githubusercontent.com/emilkowalski/skills/main/skills/review-animations/SKILL.md), and [improve-animations](https://raw.githubusercontent.com/emilkowalski/skills/main/skills/improve-animations/SKILL.md).

Application of those instructions is visible in this handoff:

- The audit is one coherent product system with a surface inventory and no more than three root findings.
- Every finding includes Contract, Runtime, and Correction evidence.
- The design document holds normative tokens and component rules; the plan holds audit evidence and implementation work.
- Motion is reviewed by frequency and purpose, with concrete property/value corrections, reduced-motion behavior, and no speculative animation.
- The existing Vanilla TypeScript/CSS stack is respected; framework-specific baseline advice is treated as non-applicable.

## Surface inventory

| Surface | Current owner / path | Contract for implementation |
| --- | --- | --- |
| Window chrome | `.app`, `.sidebar`, `.topbar`; `index.html:1-67`; CSS `1323-1660` | One 54px workspace row below the OS-owned title-bar area; continuous 168px rail; no fake traffic lights |
| Discovery navigation | Workspace nav in `index.html:7-32`; `switchView()` in `main.ts:3332-3405` | Shared 34px rail rhythm; active state is text + neutral surface + blue accent, not a card stack |
| Discovery toolbar | `index.html:34-67`; CSS `.topbar` and toolbar actions | Title and global actions only; Library Search is hidden, not a disabled duplicate |
| Today / recommendations | `renderRecommend()` `main.ts:1716-1758`; `index.html:69-76` | Daily status, 推荐/缺摘要 segments, ranked cards, and one recovery action for missing abstracts |
| Recommendation history | `renderRecommendHistory()` `main.ts:1761-1790`; `index.html:78-81` | Snapshot context is explicit; history cards reuse paper-card and state rules |
| All Papers / Favorites | `renderPapers()` and `renderFavorites()` `main.ts:1681-1800`; `index.html:83-118` | Filters remain compact metadata controls; cards preserve title → abstract → status → next action order |
| Journals / Collections | `index.html:135-164`; catalog CSS `690-812,792-856` | Catalog, search, subscription, and manual add use the same control/radius/feedback roles |
| Research Interests / Tags | `index.html:166-179`; tag CSS `905-970` | Draft/save/dirty state is local to the editor; tag color is supplemental, never the only meaning |
| Toolbar / Sidebar | CSS effective tokens `1525-1655`; Library nav `main.ts:2236-2267` | Rail 168px, row 34px, section gap 24px, counts in a 30px tabular column |
| Library Search | `librarySearch.ts`; `main.ts:2024-2234`; `index.html:35-47`; CSS `1662-1696` | One `all` mode; instant typing/IME/keyboard/result updates; anchored Collection/Tag/Paper/action suggestions; zero-count Tags remain dimmed |
| Paper Table | `main.ts:2281-2339`; `index.html:120-130`; CSS `1355-1429` | 27px header, 30/39px rows, shared grid, neutral selection, ellipsis, no page-level horizontal overflow |
| Attachments / drag-drop | `main.ts:2316-2323,2439-3025`; `index.html:125-126`; CSS `1459-1498` | PDF is a child of a Paper; managed/linked/missing/processing/error states are named; drop feedback stays local |
| Inspector | `main.ts:2358-2388`; `index.html:128-130`; CSS `1376-1437,1651-1655` | Continuous metadata surface; serif title/abstract; Citation, Library, Abstract, PDF, Citation Format groups; only Metadata tab in v0.2.1 |
| Settings | `index.html:181-290`; CSS `1280-1320`; settings handlers in `main.ts` | 680px max content; grouped Update/DeepSeek/AI/Abstract/PDF/Sync controls; field-local success/error text |
| Popovers / menus | Work status `index.html:49-58`; column menu `60-64`; Search suggestions `41-47`; relation editors in `main.ts:2341-2350` | Trigger-owned, bounded, keyboard-closable, no focus theft, no high-frequency entrance animation |
| Dialogs | `index.html:308-325`; `showConfirmModal()` `main.ts:3572-3614`; `showPromptModal()` `3616-3650` | One dialog contract with explicit destructive copy, predictable Escape/overlay behavior, focus entry and invoker restoration |
| Empty / loading / error / success | Renderers `main.ts:1681-1800,2281-2388,3187-3312`; `setStatus()` `618-624` | Keep context, name the state, provide one next action, and assign the message to the owning surface |
| Buttons / tags / chips | Generic CSS `166-201,257-282`; Library CSS `1397-1412,1501-1521`; settings token chips `1302-1307` | Primary/ghost/small/icon roles; visible focus; tags and collections differ by semantics, not only hue |
| AI status / Activity | `STATUS_ZH` `main.ts:3056-3090`; Work Center `3093-3135`; Activity `3187-3312` | Toolbar is compact status entry; Activity is the detail surface; status text and action are always paired |

## Root findings

The following three findings are the maximum coherent set for this audit. Each is a system-level correction that covers the surface inventory without turning every component into an isolated redesign request.

| # | Problem | Evidence: Contract / Runtime / Correction | Proposed change | Scope | Confidence |
| --- | --- | --- | --- | --- | --- |
| 1 | App-wide chrome and token ownership are not yet converged | Contract: the current `DESIGN.md` was Library-only and the accepted Library baseline is 168px rail / 54px workspace row. Runtime: base CSS starts with one token set at `styles.css:1-12`, historical overrides set a 46px `.topbar` at `1323-1335`, while `body.library-workspace .topbar` becomes 54px at `1635-1645`; the sidebar also moves from historical 190/224px values to the effective 168px rule. Correction: `DESIGN.md` now establishes one full-app semantic token layer and a 54px chrome contract. | Consolidate future CSS work behind the `DESIGN.md` roles; make the shared chrome, rail, content inset, control heights, radii, borders, and surfaces single-owner tokens. Keep only explicit Library table/Inspector exceptions. | `DESIGN.md`, future `app/src/styles.css` token migration, chrome/sidebar/toolbar implementation | High |
| 2 | Background work, command feedback, and local state feedback have overlapping visual ownership | Contract: a background task needs a compact persistent status, while a field error/success needs local explanation and action. Runtime: `setStatus()` mounts a bottom transient status with a 5-second dismissal at `main.ts:618-624`; the toolbar separately exposes `#work-status` and a status popover at `index.html:49-58`; Discovery, Library, Settings, and Activity each emit their own empty/error/status copy in `main.ts:1681-1800,2143-2144,3187-3312`. Correction: the plan and `DESIGN.md` assign work status to the toolbar/Activity, command confirmation to a short toast, and validation/recovery to the owning field or region. | Implement one state ownership matrix and normalize inline status placement/copy. Do not remove useful existing feedback; remove duplication and ensure each error has a nearby recovery action. | All Discovery/Library/Settings/Activity state surfaces; no backend change | High |
| 3 | Accessibility and motion rules are local rather than global | Contract: every keyboard control needs a visible focus target and reduced motion must preserve information without movement. Runtime: only selected Library controls declare `:focus-visible` (`styles.css:1141,1194,1201,1227,1380`); there is no `prefers-reduced-motion` rule; the sole non-disabled transition is `drop-queue-progress span { transition: width .2s ease; }` at `1258`, while Search intentionally disables animation at `1662-1680`. Dialog logic focuses Cancel at `main.ts:3612` but does not encode focus trapping or invoker restoration. Correction: `DESIGN.md` defines a global focus/reduced-motion contract and transform/opacity-only panel motion; the implementation handoff calls out the exact CSS and dialog changes. | Add global `:focus-visible` and reduced-motion rules; keep Search/keyboard/result changes instant; replace progress width interpolation with instant or `scaleX`; add dialog focus entry/return; test keyboard and reduced-motion paths. | All interactive surfaces; no visual animation added to high-frequency paths | High |

## Target system to implement

`DESIGN.md` is the normative source for values. The implementation should expose semantic CSS custom properties once, then consume them in the components below.

| Role | Target | Use |
| --- | --- | --- |
| App shell | `#f5f6f8` | Body/app canvas around persistent surfaces |
| Main surface | `#ffffff` | Toolbar, Discovery content, Library table |
| Sidebar / Inspector | `#f6f6f6` / `#f5f5f5` | Continuous navigation rail / reading metadata surface |
| Primary text | `#303030` | Titles and important labels |
| Secondary / muted text | `#898989` / `#aaaaaa` | Metadata, placeholders, helper copy |
| Accent / focus | `#287cff` / `#3487ff` | Active, linked, focused, and local drag-target state |
| Default / subtle border | `#e3e6ea` / `#e9e9e9` | Cards/controls vs separators |
| Library geometry | 168px rail, 54px chrome, 27px header, 30/39px rows, 300px Inspector | Preserve the supplied Library evidence |
| Search geometry | 430px max, 250px min, 28px high, 6px radius | Preserve the RC2 all-field Search contract |

Do not mechanically replace every historical literal in one pass. Migrate by owner, verify computed styles, and keep the Library screenshot baseline stable while Discovery/Settings receive the same roles.

## State contract

| State | Visual owner | Required content | Motion |
| --- | --- | --- | --- |
| Empty | The empty list/card/Inspector region | One sentence explaining absence; one clear next action when possible | None |
| Loading | The region being read or the background-work status | Keep title and controls; show `读取中…`, `分析中`, or live progress | None; no skeleton unless a later product decision adds one |
| Partial / missing | Paper card, abstract area, attachment row, or field | Warning role, retained record, source explanation, recovery action | None |
| Error | Owning field/region plus optional transient status | Human-readable cause, retry/relink/test action, no fake success | None |
| Success | Owning field/region or short status toast | Confirm what changed and leave the result visible | None; do not hold input for toast dismissal |
| Background work | Toolbar `#work-status` → Activity detail | State, count/progress, pause/stop/retry when supported | Progress instant or transform-only |
| Unsaved tags | Tag editor local header/action bar | Dirty marker plus Save/Discard decision on navigation | None |

Specific behavior to preserve:

- Missing/partial abstracts remain visible; partial is warning/amber, not error red.
- `analysisFailed` exposes retry; `waitingForAbstract` exposes recovery where the source path supports it.
- A failed updater, PDF action, or Settings validation reports beside its owning control and may also use the transient status region.
- A successful command must not replace the resulting row/card with a success-only surface.

## Motion handoff

### Before / After / Why

| Before | After | Why |
| --- | --- | --- |
| Global `.topbar` resolves to 46px in the effective RC3 block, with a 54px Library-only override (`styles.css:1330,1637-1645`) | One 54px workspace row token for Discovery and Library; OS title-bar space remains outside the CSS chrome | Spatial consistency across mode switches; the accepted Library baseline remains the reference |
| `#status` bottom toast and toolbar `#work-status` both communicate work-related state (`main.ts:618-624`, `index.html:49-58`) | Toolbar owns persistent background-work status; toast confirms a short command; inline field/region owns validation and recovery | Clear ownership reduces duplicate attention and makes errors actionable |
| Search suggestion open/close and keyboard/result updates are intentionally static (`styles.css:1662-1680`) | Keep them static; do not add spring/fade motion to high-frequency paths | Search is a functional desktop input path and must feel immediate |
| PDF queue progress interpolates `width` over `.2s ease` (`styles.css:1258`) | Set progress width instantly or render a transform-based `scaleX` indicator, gated by reduced motion | Avoid layout/paint animation and preserve honest progress feedback |
| Modal code focuses Cancel on open but has no explicit invoker restoration (`main.ts:3572-3614`) | Capture invoker, move focus inside, contain Tab, close on Escape/overlay, restore invoker | Predictable keyboard spatial consistency without adding visual motion |

### Animation review

| # | Severity | Category | Location | Finding | Fix summary |
| --- | --- | --- | --- | --- | --- |
| 1 | P1 | Layout-property animation | `app/src/styles.css:1258` | The only active transition interpolates a progress bar's `width` | Make updates instant or animate only a transform; add reduced-motion fallback |
| 2 | P1 | Reduced motion | `app/src/styles.css` global rules | No `prefers-reduced-motion` path exists, even though the design contract requires one | Add a global media query that removes movement and preserves color/text/progress state |
| 3 | P1 | Keyboard interaction | `app/src/main.ts:3572-3614` | Dialog focus enters at Cancel but focus containment and restoration are not encoded | Implement focus entry/Tab containment/return-to-trigger; keep dialog transitionless unless a later product decision authorizes one |
| 4 | P2 | Focus affordance | `app/src/styles.css:1141,1194,1201,1227,1380` | Focus styling is limited to selected Library controls and some local hover-revealed actions | Add one global visible ring, then retain local geometry-specific focus styling where it adds clarity |

## Future Inspector metadata/annotation contract

The current Inspector has one visible `元数据` tab. It owns canonical citation metadata, effective/personal Library overrides, relations, abstract language, and attachment provenance. A future `标注`/Annotation tab is a sibling view owned by the selected paper, not by the global Library or the current Search query.

The conceptual future record is:

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

The tab is rendered only when the annotation capability and provider are available. A future zero-record view has one create/import action; it is not an empty tab in v0.2.1. Annotation selection must not change paper identity, canonical metadata, or attachment ownership. This is UI/data documentation only: no annotation extraction, persistence, schema, migration, or visible tab is part of this handoff.

## Implementation sequence

1. **Token foundation and chrome.** Add the semantic tokens from `DESIGN.md` at one CSS owner, normalize the 54px workspace row and 168px rail across both modes, and preserve the native title-bar boundary.
2. **Shared interaction primitives.** Normalize button roles, icon-button labels, field borders, segmented controls, tags/chips, cards, banners, empty states, status roles, focus-visible, and disabled semantics. Keep the current copy and backend contracts.
3. **Discovery surfaces.** Apply the shared primitives to Today, recommendation history, All Papers, Favorites, Journals, Research Interests, and Activity. Keep the existing data order, filters, and actions; this is a visual/feedback refactor.
4. **Library surfaces.** Preserve the accepted Library screenshot geometry and interaction assertions while consuming the shared tokens. Audit Search, table, attachment children/queue, drag feedback, column menu, and Inspector as one information surface.
5. **Settings and dialogs.** Apply grouped settings roles, field-local validation, updater/PDF warnings, modal focus behavior, trigger-owned popovers, and narrow-window stacking.
6. **Motion and accessibility verification.** Verify no high-frequency animation, no `transition: all`, no layout-property motion, global focus visibility, reduced motion, accessible names, live-region behavior, Escape/return-focus behavior, and color-independent status.

## Acceptance checklist for implementation

- [ ] At 1512px the Library retains the supplied evidence: 168px rail, 300px Inspector, 27px header, 30/39px rows, thin separators, neutral selected row, continuous Inspector, and no horizontal overflow.
- [ ] At 1000px the Inspector collapses as documented; at 740px/600px secondary columns hide without losing Title/Journal/Year; narrow Inspector opens as a closable overlay.
- [ ] Discovery and Library share the 54px workspace row, rail rhythm, type roles, accent roles, control radii, focus ring, and state semantics.
- [ ] Search remains one all-field mode, IME-safe, instant, keyboard accessible, and free of open/close/result animation; zero-count Tags remain visible and dimmed.
- [ ] Paper cards, recommendation history, Journals, Tags, Settings, Activity, popovers, dialogs, empty/loading/error/success states, buttons, tags/chips, collections, AI status, and attachments each map to the state and surface contract.
- [ ] PDF processing feedback exposes a real progressbar and never animates `width`; missing files explain the recovery path.
- [ ] Dialogs and popovers have predictable focus, Escape, overlay, and return-to-trigger behavior.
- [ ] No `标注`/Annotation tab is rendered or advertised in v0.2.1; only the future contract exists.
- [ ] No source, dependency, schema/migration, release/tag, or user-data changes are part of the visual implementation commit.

## Explicit non-goals for this handoff

This audit does not modify `app/src/main.ts`, `app/src/styles.css`, or `app/index.html`; it does not change Search behavior, AI/recommendation scoring, PDF storage, updater/data-preservation behavior, SQLite, DB v19, migrations, releases, tags, or user data. The future Annotation contract is documentation only. The next implementation task may edit the app sources in the sequence above, but it must keep the behavior and data boundaries intact.
