# 02 Shell, sidebar, command palette

Source of truth: `forks/t3code/apps/web/src` (paths below are relative to it unless they start with `apps/` or `packages/`). Linux desktop is the target, so every value is the one Electron on Linux renders at a viewport ≥ 768px wide (`md`) and ≥ 640px (`sm`), unless a row says otherwise.

Conventions:

- Tailwind units converted: 1 unit = 4px, `rem` = 16px. `text-3xs` 10px, `text-2xs` 11px, `text-xs` 12px, `text-sm` 14px, `text-base` 16px, `text-xl` 20px, `text-2xl` 24px, `text-3xl` 30px (`index.css:185-192`).
- Colors are token names (`sidebar-row-hover`, `muted-foreground`, …). `X/NN` means token X at NN% alpha. The tokens spec resolves them.
- Radii: `--radius` 8px. `rounded-sm` 4px, `rounded-md` 6px, `rounded-lg` 8px, `rounded-xl` 12px. `--control-radius` 8px (`index.css:102, 295-300, 1148`).
- Font: `--font-sans` = `-apple-system, BlinkMacSystemFont, "Inter Variable", "Segoe UI", system-ui, sans-serif`. On Linux that resolves to the bundled Inter Variable (`main.tsx:4`). Mono: `ui-monospace, "SF Mono", Menlo, Consolas, "Liberation Mono", monospace`.
- Buttons at desktop width use their `sm:` size (`components/ui/button.tsx:16-41`): `icon` 32×32, `icon-sm` 28×28, `icon-xs` 24×24 (svg 14), `icon-micro` 20×20 (radius 4, svg 12), `default` h32, `sm` h28 px9 gap6, `xs` h24 px7 text 12. All buttons: radius 8, 1px border, `font-medium`, text 14, gap 8, active scale 0.97, disabled opacity 0.64. Variants used by the shell: `default` (bg `primary`, text `primary-foreground`, hover `primary/90`), `outline`, `ghost` (text `foreground`, hover bg `accent`), `ghost-muted` (text `muted-foreground`, hover bg `accent` + text `foreground`), `destructive`.
- Tooltip (`ui/tooltip.tsx:150-170`): bg `popover`, 1px `border`, radius 6, text 12 `popover-foreground`, leading snug, padding 4×8, max-width 320, `sideOffset` 4, open/close fade + scale 0.98. Sidebar rows use 150ms open delay, 0 close delay (`Sidebar.tsx` `TooltipProvider delay={150}`).
- Kbd (`ui/kbd.tsx:12-22`): h20, min-w20, px4, radius 4, text 12 medium sans, `muted-foreground`, bg `foreground/7%`.

---

## 1. App shell

Sources: `routes/__root.tsx:140-246`, `routes/_chat.tsx:12-31`, `components/AppSidebarLayout.tsx:317-510`, `components/ui/sidebar.tsx`, `components/threadSidebarWidth.ts`, `components/WorkspacePageHeader.tsx`, `components/WorkspacePageContainer.tsx`, `workspaceTitlebar.ts`, `index.css:90-167, 1852-1873`, `apps/desktop/src/window/DesktopWindow.ts:55-97, 335-357, 494-499`, `apps/desktop/src/settings/DesktopAppSettings.ts:54-68`.

### 1.1 Window

| Item | Value |
|---|---|
| Default size | 1100 × 780 |
| Minimum size | 400 × 360 |
| Initial background | `#000000` dark, `#ffffff` light (before first paint) |
| Frame (Linux) | `titleBarStyle: "hidden"` + `titleBarOverlay {height: 40, color: #01000000, symbolColor #1f2937 light / #f8fafc dark}`. Native min/max/close are drawn by Electron at the top right, over a transparent strip. Menu bar auto-hidden |
| Frame (macOS, for reference) | `hiddenInset`, traffic lights at x16, y = 52/2 − 7 |
| Topbar height `--workspace-topbar-height` | **40px on Linux** (`.wco` = `max(40px, titlebar-area-height)`), 52px on macOS/web |
| Window-controls inset on the right `--workspace-native-controls-inset` | width of Electron's control overlay + 12px. Tern draws its own Linux controls; reserve their width + 12 |
| `--workspace-controls-left` | 12px on Linux (`titlebar-area-x` 0 + 12). macOS: 90px for the traffic lights unless fullscreen |
| `--workspace-titlebar-control-size` | 28px |
| `--workspace-titlebar-control-gap` | 12px |
| `--workspace-titlebar-content-left` | controls-left + 28 + 12 = **52px on Linux** |
| `--workspace-gutter` | 20px at ≥640 (12px below) |

Drag regions: any element with `drag-region` drags the window; buttons, inputs, textareas, selects and links inside it do not (`index.css:1852-1873`). Drag regions: the sidebar chrome header, the chat column header, `WorkspacePageHeader` when `electron`. Everything fixed in the titlebar strip (sidebar toggle, right titlebar controls) is no-drag.

### 1.2 Tree

```
ToastProvider > AnchoredToastProvider
  hosts (dialogs: confirm, add-project, worktree approval, snooze-custom, …)
  CommandPalette (provider; renders the dialog)
    SidebarProvider  [flex row, h = window, data-sidebar-state=expanded|collapsed]
      Sidebar (left, offcanvas, resizable)  -> ThreadSidebar + SidebarRail
      <route content>  = SidebarInset (main, flex-1, bg background)
          chat route: [chat column (header, timeline, composer, terminal drawer)] [right panel]
      SidebarControl (fixed toggle button in the titlebar strip)
```

`/welcome` renders the shell without the sidebar. `/pair`, `/connect*` render bare (out of scope).

### 1.3 Horizontal layout

| Region | Width | Notes |
|---|---|---|
| Sidebar | `--sidebar-width`, default **256**, min **max(208, brand width)**, max **viewport − 640** (never below min) | Persisted in localStorage `chat_thread_sidebar_width`; open state in `chat_thread_sidebar_open` (default open). A stored width below 208 is raised to 208 on load. 1px right border (`border` inside `[data-app-sidebar]`, which is `zinc-200` light / `white/8%` dark) |
| Main column (`SidebarInset`) | flex-1, min 640 enforced by the resize guard | bg `background`. The sidebar "gap" div reserves `--sidebar-width` in the flex row; the visible sidebar is `position: fixed; left: 0; top: 0; height: 100%` over that gap |
| Right panel (in chat route) | default **540**, min **360**, max **min(70% of viewport, row width − 360)** | Persisted per thread `t3code:preview-panel-width:<threadKey>`. Below a viewport width of **980px** it becomes an overlay sheet instead of an inline column (`rightPanelLayout.ts`, `chat/ChatRightPanel.tsx`) |
| Thread details panel width token | 280 (`--thread-details-panel-width` 17.5rem) | |

"Brand width" (`SidebarBrandWidthProbe`, `sidebar/SidebarChrome.tsx:100-125`): measured width of `ml 52 + "T3 Code" wordmark + 4 gap + 2×28 (back/forward) + pr 12 + 1px border`. On Linux this is well under 208, so the min is 208.

### 1.4 Vertical layout (chat route)

| Region | Height |
|---|---|
| Chat header | `--workspace-topbar-height` (40). `drag-region`, px 20 (≥640), items centered. Holds the breadcrumb (project favicon 14 + project name in `muted-foreground` › thread title 14 medium `foreground`) |
| Timeline + composer | flex-1 |
| Terminal drawer | inside the chat column only (not under the right panel). Default **280**, min **180**, max **min(75% of window height, column height − header − composer)** (`types.ts:34`, `ThreadTerminalDrawer.tsx:113-141`) |

Content lane: chat content max width 736 (`--chat-content-max-width` 46rem); lane side padding 48px at ≥640, 12px below (`index.css:2659-2690`).

### 1.5 Titlebar strip

Everything in the strip is centered vertically in the 40px topbar.

| Element | Position | Size | Content |
|---|---|---|---|
| Sidebar toggle (`SidebarControl`) | fixed, left = controls-left + 1 = **13px**, top 0, z 50 | button 28×28, `ghost`, icon 16 | `panel-left-close` when the sidebar is open, `panel-left` when closed. Tooltip below: "Toggle main sidebar (Ctrl+B)". aria-pressed = open. Stays put whether the sidebar is open or not |
| Sidebar header brand | in the sidebar header, margin-left 52 | h28, radius 6 | "T3" wordmark (SVG, height = cap height) + "Code" 14 medium tracking-tight in `muted-foreground`, gap 4. Click → `/`. Replace with "Tern" |
| Back / Forward | right after the brand, gap 4 | `icon-sm` 28×28 `ghost`, icon 14 | `arrow-left`, `arrow-right`; disabled when history can't move. Tooltips "Go back (Ctrl+[)" / "Go forward (Ctrl+])" |
| Chat header | main column, left padding 20 when the sidebar is open | | When the sidebar is **collapsed** the header's left padding becomes `--workspace-titlebar-content-left` (52) so the title clears the fixed toggle |
| Right titlebar controls (`chat/PanelLayoutControls.tsx:201`) | fixed, right = `--workspace-controls-right` (native controls width + 12), z 50, gap 4 | | right-panel toggle buttons + maximize toggle (shown only while the right panel is open). Detailed in the chat spec |

`WorkspacePageHeader` (non-chat pages): same 40px height, gap 12, pl/pr = gutter, `drag-region`, `pr = native-controls-inset` when reserving controls, collapsed-sidebar left inset as above. `WorkspacePageContainer`: centered column, max width 896 (`readable`) / 1024 (`wide`) / 1152 (`expanded`), px 24, pt 24, pb 48, gap 24.

### 1.6 Sidebar resize, collapse, animation

Sources: `ui/sidebar.tsx:31-36, 284-460, 482-724`, `AppSidebarLayout.tsx:420-510`.

| Behaviour | Spec |
|---|---|
| Rail | invisible 16px-wide hit strip centered on the sidebar's right edge (x = W−8 … W+8), full height, z 20, cursor `col-resize`. On hover a 2px line in `sidebar-border` shows at the edge. Pointer drag resizes |
| During drag | only the sidebar and its gap change width (no transition); `--sidebar-width` and storage are written on release |
| Accept rule | growing is refused when `window width − next width < 640`; shrinking is always accepted; clamp to [min, max] |
| Drag-to-collapse | if the raw (unclamped) width drops below `min × 0.25` (52px at min 208), the sidebar shows as collapsing (`data-sidebar-drag-state=collapsed`) and collapses on release. Dragging back above that cancels |
| Double-click rail | resets to 256 and clears the stored width |
| Collapsed | `offcanvas`: gap width → 0, container `left: −width`; container is `inert`. Focus inside moves to the toggle button |
| Restore handle (collapsed) | fixed, left 8, vertically centered, 16×64, z 41. Hidden until the mouse is near; then its left half (8×64, radius 6, `sidebar-border`) shows. Click opens. Dragging right ≥ 48px live-previews the sidebar at the dragged width and restores at that width on release. Mouse only |
| Toggle | `sidebar.toggle` shortcut (Ctrl+B), the titlebar button, or the app menu. Ignored while the terminal has focus (tmux prefix), while focus is in a keybinding-capture field, and Ctrl+B in the rich-text composer with a selection bolds instead |
| Animation | width/left transition, `ease-out`, duration = setting `panelAnimationDurationMs` (0–400, **default 0 = no animation**). Disabled under reduced motion and for the 2 frames after a route change. The header's left padding animates with the same duration |
| Narrow window (< 768px) | sidebar becomes a left sheet overlay, width = viewport − 12px; toggle opens/closes the sheet. The titlebar toggle hides while the sheet is mounted (the sheet has its own) |

---

## 2. Sidebar

Sources: `components/Sidebar.tsx` (render at `:3085-3831`), `components/Sidebar.logic.ts`, `components/sidebar/SidebarChrome.tsx`, `sidebar/SidebarThreadHeader.tsx`, `sidebar/SidebarThreadRow.tsx`, `sidebar/SidebarListMarkers.tsx`, `sidebar/SidebarDraftBlock.tsx`, `sidebar/SidebarProjectFolderHeader.tsx`, `sidebar/SidebarThreadUndoNotice.tsx`, `ui/sidebar.tsx:735-1015`, `ui/collapsible-section-header.tsx`, `lib/sidebarTree.ts`, `threadActionMenu.logic.ts`, `packages/client-runtime/src/state/threadSort.ts`, `packages/client-runtime/src/state/threadInbox.ts`, `packages/contracts/src/settings.ts:75-99, 485-487, 547-577`.

The current sidebar is **not** a per-project tree. It is a single inbox list with sections (Pinned, Active, Working, Snoozed, Settled), a project scope filter, and per-row status. The old project-grouped layout survives as the Labs setting **"Boring sidebar (group by project)"** (`boringSidebarEnabled`, default **off**), which folds the Active section into project folders. Build the default first; folders are section 2.6.

### 2.1 Sidebar-local geometry tokens

| Token | Value |
|---|---|
| `--sidebar-content-inset` | 8px |
| `--sidebar-row-content-inset` | 10px |
| `--sidebar-control-gap` | 8px |
| `--sidebar-icon-color` | `sidebar-muted-foreground` mixed 85% with `sidebar` |
| Background | `sidebar` (+ the app grain texture) |
| Text | `sidebar-foreground` |

`[data-app-sidebar]` re-scopes the palette (`index.css:1267-1310`): light `sidebar` zinc-50, hover zinc-25, active/selected white, border zinc-200; dark `sidebar` = `card` (#000), hover `contrast-foreground` 8%, active 11%, selected 7%.

### 2.2 Structure, top to bottom

```
SidebarChromeHeader     h 40 (topbar), drag-region, pr 12
SidebarGroup (fixed)    padding 8, z 1
  SidebarThreadHeader   [search field][project scope][new thread]
SidebarContent          scroll area, scrollbars hidden, 12px scroll fade top/bottom
  SidebarGroup flex-1   padding 8
    draft rows + divider
    list: pinned-header marker, pinned rows, pinned-divider marker,
          active-placeholder marker, active rows,
          [Working header + rows], [Snoozed header + rows],
          Settled header, settled-placeholder marker, settled rows, "Show N more"
SidebarFooter           px 8, pt 4, pb 8, gap 8
  utility row           [settings][pull requests][issues][usage] [undo notice] [update pill]
```

List `ul`: flex column, gap 1px. The Working/Snoozed/Settled headers carry `margin-top: auto` on whichever comes first, so the shelves sit at the bottom of the viewport when the list is short.

### 2.3 Header row (`SidebarThreadHeader.tsx:59-127`)

| Part | Spec |
|---|---|
| Container | flex row, items center, gap 4 |
| Search field | flex-1, h32, radius 6, px 8, py 6, gap 8, text 14 medium `sidebar-muted-foreground`. Hover/focus-within: bg `sidebar-row-hover`, text `sidebar-foreground`; focus-within adds a 1px inset ring `ring/70`. `search` icon 16 in `--sidebar-icon-color`. Input is unstyled (no padding, 14 medium, placeholder "Search" in `sidebar-muted-foreground`). When searching, a clear button (`icon-micro` 20×20, `ghost-muted`, `x` icon 12) at the end |
| Project scope trigger | shown only when there is ≥1 project. 28×28 `SidebarMenuButton size=icon`: radius 8, icon in `--sidebar-icon-color`, hover bg `sidebar-row-hover`. Content: one scoped project → its favicon 16; two or more → first two favicons at 14, the second overlapped −4px with a 2px `sidebar` ring and 25% radius; none → `folder` icon 16. Tooltip "Projects" or "Projects: a, b" |
| New thread button | 28×28, same style, `square-pen` icon. Disabled with no projects. Tooltip "New thread (Ctrl+N)" style; when the click would open a picker the tooltip adds a second muted line "New thread in current project: Shift+click (Ctrl+Shift+N)" |

New-thread click logic (`Sidebar.tsx:3044-3076`, `Sidebar.logic.ts:816-851`): scope filtered to exactly one project (default sidebar only) → new draft in that project (prefer the viewed thread's environment). Else if Shift held, or only one project open, or zero projects → new thread in the current context (viewed thread's project, else default), falling back to the palette's "New thread in…" picker. Otherwise open the palette at "New thread in…".

**Project scope popup** (`Sidebar.tsx:3092-3232`): combobox popup aligned start, width min(288, available), flips. Search input "Search projects..." on top; "No matching projects." when empty. Items in order: "All projects" (`folder` icon), each project (favicon 16 + name 14 truncate + optional environment badge + row actions: pin toggle and a `settings` `icon-xs` `ghost-muted` button with a 14px icon), "Add project" (`folder-plus`). Selection is shown by a badge on the row icon (`ProjectScopeRowIcon`), not a row highlight. Click selects only that project; Shift+click or Shift+Enter toggles it in a multi-selection; "All projects" clears. "Add project" opens the palette's add-project page. The `thread.pin` shortcut (Ctrl+Shift+P) pins the highlighted project; ContextMenu or Shift+F10 opens project settings. Filtering by query hides "All projects" and "Add project".

### 2.4 Sections and sorting

Section of each visible thread (`Sidebar.logic.ts:203-213`, first match wins): **snoozed** (snooze time in future) → **settled** (`settledOverride == "settled"`) → **pinned** (`pinnedAt` set) → **active**. Active threads that are "working" go to the **Working** shelf when the setting `sidebarWorkingShelfEnabled` (default **on**) is on and folder mode is off. Visible = not archived, not a sub-thread shown under its parent, and inside the project scope.

"Working" (`threadInbox.ts:isThreadWorking`): no pending approval/input/subagent request, runtime active or idle, and not a plan-mode thread with an actionable proposed plan whose run settled. A thread leaving Working gets a 2s grace before it moves back (`WORKING_EXIT_GRACE_MS`).

| Section | Sort | Header | Default expanded |
|---|---|---|---|
| Pinned | `pinOrderKey` ascending (fractional keys written by drag), then keyless by `createdAt` desc, tie id then environment id | none (a "Pinned" boundary label appears only while dragging) | always |
| Active | Working shelf on: newest of `createdAt`, `unsettledAt`, `latestRun.requestedAt`, `latestRun.completedAt`, observed return time; desc. Shelf off: `activeOrderKey` (manual drag order) with implied keys from max(`createdAt`, `unsettledAt`) newest first. Folder mode: by send time (as Working) | none | always |
| Working | max(`createdAt`, `latestUserAuthoredMessageAt` ‖ `latestRun.requestedAt`) desc | "Working" + count | **collapsed** (`t3code:sidebar:working-expanded`) |
| Snoozed | `snoozedUntil` ascending | "Snoozed" + count | collapsed (`t3code:sidebar:snoozed-expanded`) |
| Settled | `settledAt` (fallback: activity timestamp) desc, tie id | "Settled" + count | collapsed (`t3code:sidebar:settled-expanded`). Shows 10, then "Show N more" pages of 25 |

A collapsed shelf still shows the open thread's row if it lives there. Ties everywhere break on id then environment id.

Activity timestamp (`threadSort.ts:41-58`), used for the relative time label: latest of `latestUserMessageAt`, `latestRun.requestedAt/startedAt/completedAt`, else `updatedAt`.

Project order (used by the scope list, folders and the index page): `sidebarProjectSortOrder` default `updated_at` = newest thread timestamp in the project (latest user message), else project `updatedAt`; `created_at` uses thread/project creation; `manual` keeps stored order. Tie: title, then key (`Sidebar.logic.ts:1378-1505`). In project pickers the current project leads, ahead of everything (`sidebarProjectGrouping.ts:254`). A pinned project keeps its folder in folder mode even with no active threads (`Sidebar.tsx:1389`).

### 2.5 Thread row variants

The Active, Pinned and Working sections use **card** rows. Snoozed and Settled use **slim** rows. Folder mode uses **folder** rows.

#### Card row (`SidebarThreadRow.tsx:1543-1760`)

| Part | Spec |
|---|---|
| `li` | py 2. Total slot 78px (82 with padding) when the meta line exists, auto (~60) without |
| Surface | radius 6, full width, `select-none`, cursor pointer. Focus-visible: 2px inset `ring` |
| Inner box | h **78** (4.875rem), px 10, py 8 |
| Line 1 (h 20, gap 6) | [draft pen 12 `warning-foreground` if unsent draft] [project favicon 16] [pin indicator 12] [project name, text 12 normal `secondary-label`, truncate, flex-1] [status slot] |
| Status slot | min-w 32, h 20, right-aligned, text 12. Shows one of: "Pending" (unsynced); the status label with icon 16 (see 2.7) and, for working/waiting, a live duration; else the relative time (`secondary-label`, tabular). On row hover or keyboard focus the label hides and the row actions show in its place |
| Row actions (hover) | inline, h 20, text 12 `muted-foreground` → `foreground` on hover, px 6, radius 6: [`x` 14 "Discard draft" if unsent draft] [`clock` 12 Snooze menu] [`check` 14 + "Settle"] |
| Line 2 (mt 4, gap 6) | title: text 14, `font-medium` (or normal when receded), `foreground`, truncate; provider icon stack at the end if there is no meta line |
| Line 3 meta (mt 2, gap 6, text 12 `secondary-label`) | [environment machine icon 14 `icon-muted` + environment label, only for remote envs] or spacer, [running-terminal icon 14 teal, pulsing] [PR badge] [issue badge] [diff `+N −M` mono, `diff-addition-foreground` / `diff-deletion-foreground`] [provider icon stack, right] |

Provider icon stack: current provider icon (compact size, opacity 0.6); earlier providers in the thread's history before it at 12px, opacity 0.35, grayscale, gap 6.

PR badge: inline button, text 12 tabular, icon 12 + number (max-w 64 truncate) in the PR state tone: open `git-pull-request-arrow` emerald-600/300, draft `git-pull-request-draft` zinc-500/400, closed `git-pull-request-closed` red-600/300, merged `git-merge` violet-600/300 (`pullRequest/pullRequestIcons.tsx:34-55`). Click opens the PR (in the right panel when a thread is open, else externally). Hover shows a 320px card listing linked PRs.

#### Slim row (Snoozed, Settled) (`SidebarThreadRow.tsx:1330-1540`)

h **36**, px 10, gap 10, one line: [favicon 16; at 40% opacity + grayscale unless active/hovered/focused] [draft pen] [title 14] [pin] [terminal icon] [issue badge] [time slot]. Settled rows (`variantAction == "unsettle"`) render every child in `secondary-label` until hovered. Time slot: min-w 32, h 24; shows relative time (settled rows use the settle time), the snooze wake time (`alarm-clock` 12 + label in `info-foreground`) for snoozed rows, or a "Woke" button. Hover replaces it with the row action: Settled → `undo-2` 14 "Un-settle thread"; Snoozed → `alarm-clock-off` 12 "Wake thread now".

Slim title color: active/woke/input → `foreground`; unread → `muted-foreground`; else `secondary-label/70`; hover → `foreground`.

#### Row surface states (`SidebarThreadRow.tsx:1060-1080`)

| State | Background | Text |
|---|---|---|
| Open thread (route) | `sidebar-row-active`, light theme adds 1px inset ring `sidebar-border/70` | `sidebar-foreground` |
| Multi-selected / in a sweep | `sidebar-row-selected` (+ same light ring) | `sidebar-foreground` |
| Has unsent draft | `warning/4%`, hover `warning/8%` | `sidebar-foreground` |
| Receded (see below) | transparent, hover `sidebar-row-hover` | `sidebar-muted-foreground/75`, hover `sidebar-foreground`. Working/preparing receded rows also opacity 0.7 → 1 on hover |
| Default | transparent, hover `sidebar-row-hover` | `sidebar-foreground` |
| File drag over | 1px inset ring `primary/70`, bg `sidebar-row-hover` | |
| Being dragged | bg `sidebar-row-active`, `shadow-lg`, z 20 | |

Receded (`Sidebar.logic.ts:930-962`): never when open, selected, or status is input/request. Always when preparing/working/waiting. When ready or approval, receded unless unread or woke.

#### Inline rename

Started by double-click on the row (no modifiers, not on a button/link), F2 on a focused row, or the context menu. The title becomes an input: text 14 medium, bg `card`, text `card-foreground`, px 4 (−4 margin so the text doesn't shift), radius 4, 1px inset ring `input`, `foreground` when focused; text is selected on focus. Enter commits, Escape cancels, blur commits. Empty title → warning toast "Thread title cannot be empty". Unchanged → no-op. Failure → error toast "Failed to rename thread".

#### Hover card

Every row has a details card (`ThreadHoverCard`, right side, start aligned, offset 4): title, then 12px rows with 12px icons in `muted-foreground`: project (favicon + name), environment, branch (`git-branch`, middle-truncated), worktree path (`folder-git-2`), branch mismatch warning (`circle-alert`, `warning`), model/provider, previous providers (`arrow-right-left`), running terminals, PR mini-list footer.

### 2.6 Folder mode ("Boring sidebar", Labs, off by default)

Active threads are grouped under project folders, in project order. Pinned stays on top; Snoozed and Settled stay below; the Working shelf is disabled.

Folder header (`SidebarProjectFolderHeader.tsx`): `li` margin-top 8 (0 for the first). Button h32, radius 8, px 10, gap 10, hover `sidebar-row-hover`: [project favicon 16] [name 14 medium `sidebar-foreground/90` truncate] [status dot 6×6 round, only when collapsed: attention `warning`, failed `error`, unread `success`, working `info`] [hidden-thread count, 12 tabular `sidebar-muted-foreground`, only when collapsed; hidden on hover] [`chevron-down` 12 `sidebar-muted-foreground`, rotated −90° when collapsed]. Hover shows a `square-pen` `icon-xs` `ghost-muted` "New thread in {project}" button 24px from the right, over the count.

Dot precedence (`Sidebar.logic.ts:1032-1057`): any input/approval/request/limited → attention; any failed → failed; any ready+unread → unread; any working/preparing → working.

Folder rows: h36 one line, padding-start 36 (title aligns past chevron and icon), no favicon; the status label (icon 12 + text 12 medium in status tone) takes the time slot. Optional "Two-line folder rows" (`boringSidebarMultilineRows`) adds the meta line below.

Collapsed folder still shows the open thread. Expanded state shares `projectExpandedById` with the legacy per-project sidebar. Folder context menu: New thread · Copy path · Pin/Unpin project · Project settings. Folder rows are not reorderable (drag only drags them out as chat context).

### 2.7 Status indicators

Status resolution (`Sidebar.logic.ts:964-1005`), first match wins:

| Status | Condition | Label | Icon (lucide) | Tone |
|---|---|---|---|---|
| approval | pending approvals or subagent approval request | "Approval" | `shield-question` | `warning-foreground` |
| input | pending user input or subagent input request | "Input" | `message-circle-question` | `indigo-600` / dark `indigo-300` |
| request | guest message waiting | "Request" | `user-round` | `guest-foreground` (drop: guest sharing is out) |
| waiting | runtime running but blocked on a child thread; or runtime `idle` | "Waiting" + duration since last run completed | `circle-dot-dashed` | `muted-foreground` |
| preparing | runtime `preparing` | "Setting up" + duration | `circle-dashed` | `info-foreground` |
| working | runtime queued/starting/running/waiting | "Working" (or "Goal" when a goal is active) + live duration | `circle-dashed` | `info-foreground` |
| limited | runtime failed with `usage_limit` | "Limited" | `circle-alert` | `warning-foreground` |
| failed | runtime failed, and the failed run is unseen | "Failed" | `circle-alert` | `error-foreground` |
| woke | snooze ended after the last visit (drop with snooze) | "Woke" | `alarm-clock` | `warning-foreground` |
| done (unread) | ready and latest run completed (not interrupted/cancelled) after `lastVisitedAt` | "Done" | `circle-check` | `success-foreground` |
| ready | none of the above | (relative time) | | |

A failed run the user already opened reverts to ready. Duration ticks every second, tabular. Relative time ticks every minute; "now" for under a minute, else the compact value (e.g. "5m", "3h", "2d"; `timestampFormat.ts:388`).

Other row indicators: unsent draft (`square-pen` 12 `warning-foreground`, tooltip "Unsent draft"); pinned (`pin` 12 `muted-foreground/65`, turns into an `pin-off` unpin button on hover); running terminals (`terminal` 14, teal-600 / teal-300·90%, pulse, tooltip "N terminal process(es) running"); title regenerating (shimmer over the title, `muted-foreground`).

### 2.8 Section markers and shelves

Shelf header (`SidebarListMarkers.tsx:279-313`, `ui/collapsible-section-header.tsx`): `li` h32, mx 2. Button h32, px 8, radius 6, gap 8, text 12 medium: [label] [1px rule flex-1, hidden when a count is shown] [count, normal weight, tabular] [`chevron-down` 12, −90° when collapsed]. Tones: muted (`sidebar-muted-foreground`, rule `sidebar-border/50`), emphasized while dragging (`sidebar-foreground/80`), accent when it's the drop target (`primary`).

Drag-only markers: "Pinned" / "Active" boundary labels (h16, inset 8, text 12 medium + 1px rule; `primary` when targeted, else `sidebar-foreground/80` with a `sidebar-foreground/25` rule) and empty-section placeholders (h36 dashed 1px box `sidebar-foreground/25`, radius 6, text 12 `sidebar-foreground/80`; targeted: border `primary/40`, bg `primary/5`, text `primary`).

"Show N more" (settled): h36, px 10, radius 6, gap 10, text 14 `sidebar-muted-foreground`, `chevron-down` 16; hover `sidebar-row-hover`. Shows "Loading" / "Could not load older threads. Retry" while paging from the server.

### 2.9 Draft rows

Unsent new-thread drafts (not yet a server thread) list above everything (`SidebarDraftBlock.tsx`). Same 78px card box as a thread card: line 1 [`square-pen` 12 `warning-foreground`] [favicon 16] [project name 12 medium `secondary-label`] [hover `x` 12 "Discard draft"]; line 2 (mt 2) the prompt preview (14 medium `foreground/90` truncate), or "N attachments", or a terminal label. Surface `warning/4%` (hover 8%), or `sidebar-row-active` when open. A 1px divider (`sidebar-border/60`, mx 10, my 6) follows the block. Context menu: Copy ▸ (Path, Branch) · Project settings · ─ · Discard draft (destructive).

### 2.10 Thread context menu (`threadActionMenu.logic.ts:150-345`)

On desktop the menu is native (`desktopBridge.showContextMenu`); Tern renders its own popup menu. Items in order (separator = ─):

| Item | Icon | Shown when |
|---|---|---|
| Find in thread | `text-search` | the row is the open thread |
| New thread on {branch} | `message-square-plus` | thread has a branch |
| Pin thread / Unpin thread | `pin` / `pin-off` | server supports pinning |
| Settle thread / Un-settle thread | `circle-check` | supports settlement |
| Snooze ▸ presets…, ─, Custom… / Wake thread | `clock` | supports snooze (drop, see 7) |
| ─ Rename thread | `pencil` | always (disabled in search results) |
| Regenerate title / "Regenerating…" | `refresh-cw` | server supports it |
| Mark unread | `mail-open` | has a completed run and not running |
| Share with guest | `user-plus` | (drop) |
| Filter by {project} / Show all projects | `folder-tree` | thread has a project |
| Turn into project | `folder-plus` | no-project thread with its own folder; disabled while running |
| Auto-settle behavior ▸ Follow settings ✓ / Never | `timer` | server supports the opt-out |
| ─ Copy ▸ Path (`folder`), Branch (`git-branch`, if any), Thread (`message-square`), Thread ID (`hash`) | `copy` | always |
| Project settings | `settings` | always |
| ─ Archive thread | `archive` | disabled while running |
| Delete | `trash`, destructive | always |

Read-only clients get every operate-requiring item disabled. With several rows selected, the menu acts on all of them (bulk settle, unpin, regenerate titles, archive, delete).

Archive: confirm only if setting `confirmThreadArchive` (default **false**): `Archive thread "{title}"?`. Delete: confirm if `confirmThreadDelete` (default **true**): `Delete thread "{title}"?` + newline + "This permanently clears conversation history for this thread.", destructive variant. Errors: "Failed to archive thread", "Thread archived, but navigation failed", "Failed to delete thread", "Path unavailable / This thread does not have a workspace path to copy." Settling or archiving the open thread navigates to the next non-settled, non-snoozed row below it (wrapping), else a new draft in its project, else `/` (`Sidebar.tsx:1645-1670`).

### 2.11 Undo notice (footer)

Settle, snooze, unpin, archive and discard show a compact notice in the free space of the footer utility row (`SidebarThreadUndoNotice.tsx`): h24, text 11, leading 24, `sidebar-muted-foreground`, right-to-left: "Settled," / "Settled 3," then an inline "Ctrl+Z to undo" button (`undo-2` icon 16 shows only when the label wraps away). Fades in/out over 200ms. Expires after **5s** (`hooks/showThreadUndoNotice.ts:163-177`). `thread.undo` (Ctrl+Z) undoes the latest batch, claimed in capture phase so it beats the composer's own undo while the notice is live; holding the key undoes once.

### 2.12 Drag and reorder

Sources: `Sidebar.tsx:909-916`, `Sidebar.drag.ts`, `Sidebar.logic.ts:256-330`.

| Rule | Value |
|---|---|
| Start distance | 6px pointer movement |
| Auto-scroll | when within 5% of the top/bottom edge, acceleration 3 |
| Drop targets | Pinned (sets `pinOrderKey` between neighbours), Active (sets `activeOrderKey`; only when the Working shelf is off), Settled (settles) |
| Drop verb badge | replaces the status slot while dragging: h20, px 6, radius 4, 1px `primary/40`, bg `primary/10`, text 11 medium `primary`, icon 12: Pin (`pin`), Unpin (`pin-off`), Settle (`circle-check`), Un-settle (`undo-2`), Wake (`alarm-clock-off`) |
| Not droppable | Working rows (drag out only, as chat context), Snoozed (no wake time), folder rows |
| Drag out | dropping a row on the composer attaches the thread as context |
| File drop on a row | attaches files to that thread (ring `primary/70`) |

Action sweep (press a row's Settle/Un-settle/Wake button and drag across other rows to apply it to all of them) is in the source; skip it (section 7).

### 2.13 Selection and keyboard

| Input | Effect |
|---|---|
| Click row | open thread. A second click of a double-click on the open row does nothing |
| Ctrl+click | toggle row in the multi-selection |
| Shift+click | range-select from the anchor in visible order |
| Double-click / F2 | rename |
| Enter / Space on a focused row | open |
| ↑ / ↓ / Home / End | move focus between visible rows without opening (`lib/sidebarTree.ts`) |
| → / ← | on a collapsed shelf/folder header: expand / collapse; otherwise jump to first child / parent row |
| ContextMenu, Shift+F10 | context menu at the row |
| `thread.previous` / `thread.next` (default keys in section 5) | open the adjacent row in visible order (drafts first) |
| `thread.jump.1…9` | open the Nth visible row. Holding the jump modifier ≥ **200ms** shows a Kbd hint in each of the first 9 rows' status slot (`THREAD_JUMP_HINT_SHOW_DELAY_MS`) |
| Search field: ↑/↓, Enter, Escape | move the highlighted result, open it, clear the search |

Shortcuts are ignored while the command palette or model picker is open, and on key repeat.

### 2.14 Search

Typing in the header field replaces the list with results (`Sidebar.tsx:3344-3427`, `Sidebar.logic.ts:1142-1174`). Title matches come first, ranked by the shared search scorer (same as the palette, see 4), then threads whose message content matched on the server, in the server's order. Results include settled threads. Result row: min-h 36, px 10, py 4, radius 6, gap 10, text 14: [favicon 16, faded+grayscale for settled] [title, medium (normal for settled)] [PR number match] [relative time 12 `muted-foreground/55`, min-w 32 right-aligned], then an optional excerpt line with the matched snippet. Highlighted/open: bg `sidebar-row-active`. Empty: centered 12px `sidebar-muted-foreground`, py 24: "Searching thread messages…" / "Couldn't search thread messages" / "No threads found".

### 2.15 Footer (`SidebarChrome.tsx:203-324`)

Utility row: horizontal `SidebarMenu`, items 32×32 (`SidebarMenuButton size=icon`, radius 8, icon 16 in `--sidebar-icon-color`, hover bg `sidebar-row-hover` + icon `sidebar-foreground`), tooltip on top:

| Button | Icon | Action | Shown |
|---|---|---|---|
| Settings | `settings` | `/settings` | always |
| Pull requests | pull-request glyph (`git-pull-request-arrow`) | `/pull-requests` | when the environment's source control supports PR lists |
| Issues | issue glyph | `/issues` | when supported |
| Usage | `chart-no-axes-column` | `/usage` | always |
| (undo notice) | | | while live, fills remaining width |

On a settings/usage/PR/issues page the row becomes a single full-width `SidebarMenuButton` (h32, px 10, gap 8, text 14 medium `sidebar-muted-foreground/80`): `arrow-left` + "Back", which returns to the main app. Above the row: provider update pill and app update pill (both out of scope).

### 2.16 Empty states

In-list empty (`Sidebar.tsx:3429-3480`), centered column, gap 8, px 8, py 24, text 12 `muted-foreground/60`:

| Case | Text | Button |
|---|---|---|
| No projects | "No projects yet" | "Add project" (`plus` 12): inline-flex, gap 6, radius 6, 1px `sidebar-border`, px 10, py 4, text 11 medium `sidebar-muted-foreground`, hover bg `sidebar-row-hover` + `sidebar-foreground`. Opens palette add-project |
| Scoped, nothing active | "No active threads in {project}" / "No active threads in these projects" (if settled exist) or "No threads in {project} yet" / "No threads in these projects yet" | "Show all projects" (same style) |
| Otherwise | "No threads yet" | |

`NoProjectsHero` (`components/NoProjectsHero.tsx`), main area when there are zero projects and every environment has bootstrapped: `WorkspacePageHeader` strip, then an `Empty` (hero size: centered, gap 24, padding 48) with a max-w 512 box (px 32, py 48): title "What should we work on?" (`font-semibold`, 30px at ≥640, 24px below), description 14 `muted-foreground` mt 4 "Add a project, or start without one." (or "Add a project to start your first thread." if no scratch support), buttons row mt 24 gap 8: `sm` default "Add project" (`plus` 16) → palette add-project; `sm` outline "Start without a project" (`message-square-dashed` 16) → scratch thread.

`NoActiveThreadState` (`components/NoActiveThreadState.tsx`): header with bottom border and "No active thread" (12, `muted-foreground/50`); body `Empty` with title "Pick a thread to continue" (20 semibold) and "Select an existing thread or create a new one to get started." (14 `muted-foreground`).

---

## 3. Index, new-thread and draft screens

Sources: `routes/_chat.index.tsx`, `routes/_chat.new.tsx`, `routes/_chat.draft.$draftId.tsx`, `routes/_chat.tsx`, `components/chat/DraftHeroHeadline.tsx`, `chat/DraftHeroProjectHeading.tsx`, `chat/useDraftHeroLayoutTransition.ts`, `chat/draftHeroTransition.ts`, `ChatView.tsx:1730-1750, 3770-3830`.

| Route | Behaviour |
|---|---|
| `/` (index) | Finds the most recent non-archived project (`updated_at` order) and immediately creates a local draft thread in it, replacing the URL with `/draft/<id>`. With no projects (after bootstrap) it shows `NoProjectsHero`. If draft creation throws: `Empty` "Couldn't start a new thread" / "The project is still available. Try opening the draft again." + `sm` "Try again" with a refresh icon |
| `/draft/$draftId` | Renders nothing itself; the `_chat` layout keeps one `ChatView` mounted for drafts and threads so promoting a draft to a server thread does not remount |
| `/new?...` | Prompt deep link (`parseNewThreadLink`). Invalid → `Empty` "Could not open the link" + error. Treat as out of scope unless Tern registers a URL scheme |

**Draft hero** (`isDraftHeroState`: local draft with an empty timeline, not working, no worktree setup card): the composer is vertically centered in the chat area instead of docked at the bottom (overlay `absolute inset-0 flex items-center`; on windows shorter than 544px it docks to the bottom). The headline sits directly above the composer (absolutely positioned at `bottom: 100%`, padding-bottom 16), inside the 736px content lane.

Headline content (`DraftHeroHeadline.tsx:356-389`), centered, max-w 1024:

| Case | Text |
|---|---|
| Project chosen | "What should we build in **{project}**?" on one line, 30px normal, tracking-tight, `foreground`, project name medium, **if** it fits the container at ≥1024px; otherwise two lines: "What should we build?" (30px, 24px below 640) then the project picker (16px) under it, gap 8 |
| No project chosen, projects exist | "{Choose a project} to start" |
| No projects | "Add a project to start", with the picker being a dotted-underline button "Add a project" (`muted-foreground/60`, border-bottom 1px dotted `muted-foreground/35`; hover `/80` and `/60`) |
| Scratch (no-project) draft | "What should we work on?" and below it the picker reading "No project" |

Under the headline (when scratch threads are supported): a 24px-high row, text 14, with the inline muted button "or start without a project" (tooltip shows the `chat.newWithoutProject` shortcut).

Project picker: inline "picker" button showing the project name (single line, truncate) with a combobox popup (center aligned, width 288): "Search projects..." input, items [favicon 16][name 14][pin toggle][environment badge], last item "Add project" (`folder-plus`). "No matching projects." when empty. Choosing a project re-targets the draft.

Transition when the first message is sent: the composer animates from its centered spot to the docked spot with a FLIP translate, `cubic-bezier(0.4, 0, 0.2, 1)`, duration = `panelAnimationDurationMs` (default 0, so it snaps).

---

## 4. Command palette

Sources: `components/CommandPalette.tsx`, `CommandPalette.logic.ts`, `CommandPaletteResults.tsx`, `CommandPaletteContent.tsx`, `CommandPaletteThreadItems.tsx`, `CommandPaletteThreadActions.tsx`, `CommandPaletteProjectItems.tsx`, `CommandPaletteThemeItems.tsx`, `CommandPaletteAddProject.tsx`, `commandPaletteBus.ts`, `commandPaletteStore.ts`, `components/ui/command.tsx`, `ui/autocomplete.tsx`, `ui/dialog-styles.ts`, `packages/shared/src/paletteSearch.ts`, `index.css:452-494`.

### 4.1 Opening and state

| Key (Linux) | Command | Effect |
|---|---|---|
| Ctrl+K | `commandPalette.toggle` | open in command mode; if already open in that mode, close |
| Ctrl+P | `filePicker.toggle` | open in files mode (file picker, separate spec) |
| Ctrl+Shift+F | `projectSearch.toggle` | open in content-search mode (separate spec) |
| Ctrl+Alt+A | `theme.select` | open directly on the "Change theme" submenu |

None fire while the terminal has focus. Store (`commandPaletteStore.ts`, reducer `CommandPalette.logic.ts:119-170`): `{open, mode: command|files|content, openIntent, session}`. Each open bumps `session`, and the body remounts fresh (no stale query or highlight). Other code opens it through the bus (`commandPaletteBus.ts:16-33`): `open: "add-project"`, `open: "new-thread-in"`, or a prefilled query. Opening on an intent pre-pushes that submenu (`CommandPalette.tsx:304-328`).

Inside command mode there is a view stack `{query, views: [{view, parentQuery}]}`. Pushing a submenu sets the query to its `initialQuery` or ""; popping restores the parent's query.

| Close / back | Behaviour |
|---|---|
| Escape (not repeat) | files/content mode → back to command mode with the saved query. Command mode → close the palette, even from inside a submenu |
| Backspace on an empty query in a submenu | pop one view |
| Back button | in submenus the leading search icon becomes an `arrow-left` button that pops |
| Clearing a path-browse query to "" | pops the browse view |
| Pointer-down on the backdrop | close |
| After close | focus returns to the composer, or to whatever opened the palette |

### 4.2 Layout

| Part | Spec |
|---|---|
| Viewport | fixed, full window, z 50, flex column centered horizontally, px 16. Top padding `max(10vh, topbar + 8)`, bottom 10vh. No-drag |
| Backdrop | `background` at 60% (64% dark), 4px backdrop blur, 200ms fade |
| Popup | width 100%, **max-w 576, max-h 420**, radius 16, 1px border, overflow hidden, glass: `background` at 80% with 12px blur and 1.14 saturation (dark: 16px, 1.08). Border `foreground/10%` light, white/8% dark. Shadow `0 24px 64px -24px rgb(0 0 0/.65)` light; dark `0 24px 72px -20px rgb(0 0 0/.9)` + `inset 0 1px rgb(255 255 255/.04)`. Enter/exit scale 0.98 + opacity, 200ms ease-in-out. Content-search mode pins height to 420 |
| Input band | px 8, py 6, total **46** tall. Input h34 (line-height 34), text 14, transparent, no border, no ring, placeholder token `placeholder`. Text starts at x = 32. Leading icon `search` 16, `icon-muted` at 80% opacity, at x ≈ 9+2 |
| Placeholders | root "Search commands, projects, and threads..."; submenu "Search..."; path browse "Enter project path (e.g. ~/projects/my-app)" (root) / "Enter path (e.g. ~/projects/my-app)" (submenu) |
| Results | virtualized list, max-h min(448, 70vh) (the 420 popup cap wins), padding 8 all round, estimated row 40, 12px scroll fades. Keyboard scroll keeps 24px margin |
| Group heading | px 8, py 6, text 12 medium `muted-foreground`; every heading after the first gets +6 top padding |
| Item row | min-h 28, px 8, py 6, gap 8, radius 4, text 14. Highlighted: bg `accent`, text `accent-foreground` (the palette drives the highlight; hover alone does nothing). Disabled: opacity 0.64, skipped by arrows |
| Item icon | slot h20, top-aligned, pt 2; icon 16 `icon-muted` |
| Item text | title 14 `foreground` truncate; optional description line 12 `muted-foreground/70`. Thread rows with a snippet line also show "project · environment" next to the title (12, `muted-foreground/70`, max 45% width) |
| Item trailing, in order | trailing content, pin button (projects; 14px pin, visible when highlighted or pinned), timestamp (min-w 48, right, 12 tabular `muted-foreground/70`), shortcut, submenu `chevron-right` 16 `muted-foreground/70` |
| Shortcut text | plain text, not a key cap: margin-left auto, 12 medium, letter-spacing widest, `secondary-label`, e.g. "Ctrl+Shift+N" |
| Empty | py 40, centered, 14 `muted-foreground`: "No matching commands, projects, or threads." (or "No matching actions." with a `>` prefix) |
| Footer | 1px top border `foreground/7%`, bg `foreground/2.5%`, px 16, py 10, text 14 medium `muted-foreground`, items gap 12. Key caps h20 min-w20 px4 radius 4, 12 medium, bg `foreground/8%`, text `foreground`, icon 12, group gap 4. Left to right: `[↑][↓] Navigate`, `[Enter] {Add/Select/Create/Toggle/Continue/Lookup}` (only when the flow names one), `[⌫] Back` (in submenus), `[Esc] Close`. Right slot: "Open in Files" ghost button (12px) in path browse, or "Searching messages…" |

### 4.3 Root content

Empty query: two groups, **Actions** then **Recent Threads** (12 most recently used threads, excluding the open one; recency = later of last user message and last open, `CommandPalette.tsx:985-992`).

Actions, in order (`CommandPalette.tsx:1352-1872`). Shortcuts are Linux defaults.

| # | Title | Icon | Shortcut | Shown when | Action |
|---|---|---|---|---|---|
| 1 | New thread in **{project}** (name semibold) | `square-pen` | Ctrl+Shift+N | projects exist and a contextual project exists | new draft in that project |
| 2 | New thread in... ▸ | `square-pen` | | projects exist | submenu "Projects": every project (first 9 get Ctrl+1…9) + "No project" (`message-square-dashed`); "Add project" if there are none |
| 3 | New thread without a project | `message-square-dashed` | Ctrl+Alt+N | scratch supported | scratch draft |
| 4 | Find in current thread | `text-search` | Ctrl+F | open thread, server supports find | |
| 5 | Copy thread | `link` | Ctrl+Shift+C | on a thread | copy reference |
| 6 | Restart agent session | `rotate-ccw` | | open thread | stop session; success toast "Agent session will restart" / "Your next message starts a fresh session." |
| 7 | Thread actions for the open thread | per 2.10 | Pin Ctrl+Shift+P, Settle Ctrl+Shift+S | open thread | same list as the context menu minus Rename and Project settings; entries with children are submenus |
| 8 | Go to file | `file-search` | Ctrl+P | | files mode |
| 9 | Search project contents | `text-search` | Ctrl+Shift+F | | content mode |
| 10 | New project | `folder-git-2` | | an environment has `newProjectsRoot` | name → create git repo (optional) |
| 11 | **Add project** | `folder-plus` | | always | keeps the palette open; see 4.5 |
| 12 | Change theme ▸ | `palette` | Ctrl+Alt+A | | theme rows: `palette` icon, optional "For light mode"/"For dark mode", trailing "Current" (12 `muted-foreground/70`) + 12px swatch dots, gap 4 |
| 13 | Change appearance ▸ | `monitor` | Ctrl+Alt+Shift+A cycles | | System (`monitor`), Light (`sun`), Dark (`moon`), "Current" tag |
| 14 | Toggle theme editor | `palette` | Ctrl+Alt+Shift+T | | |
| 15 | Open usage | `chart-no-axes-column` | Ctrl+U | | `/usage` |
| 16 | Focus Next / Previous Workspace Area | `monitor` | F6 / Shift+F6 | | |
| 17 | Open settings | `settings` | | | `/settings` |
| 18 | Pin project / Unpin project | `pin` / `pin-off` | | contextual project | description = project name |
| 19 | Project settings | `folder` | | contextual project | `/projects/$key` |

Dropped actions are listed in section 7.

Choosing an item: a submenu pushes its view; anything else closes the palette (unless flagged `keepOpen`) and runs. A thrown error shows the toast "Unable to run command".

**Thread row** (`CommandPalette.logic.ts:439-528`, `CommandPaletteThreadItems.tsx:116-200`, `ThreadCommandSubtitle.tsx`): icon `message-square`; before the title a 12px PR-state icon and the status pill; second line (12 `muted-foreground/70`, parts joined by "·" in `muted-foreground/50`): project favicon 12 + name, environment label, branch (12px `git-branch`/`folder-git-2`/`folder` icon, middle-truncated), provider icon 12 at 70%, "Current thread". Timestamp from `latestUserMessageAt ?? updatedAt ?? createdAt`: "just now" (< 60s), "{n}m ago", "{n}h ago", "{n}d ago" (`timestampFormat.ts:362-370`). Archived threads are hidden; the open thread sorts last in its group. Choosing navigates to the thread.

**Project row** (`CommandPaletteProjectItems.tsx`): favicon icon, display name, description "Current · Local · {workspace root}". Choosing opens the project's latest unsettled, unsnoozed thread, else starts a new draft in it.

### 4.4 Filtering and ranking

Entry `filterCommandPaletteGroups` (`CommandPalette.logic.ts:575-701`); scorer `packages/shared/src/paletteSearch.ts`. The sidebar thread search uses the same thread scorer.

| Rule | Spec |
|---|---|
| Empty query | groups unchanged (Actions, Recent Threads) |
| `>` prefix | search Actions only |
| Typed query at root | drop Recent Threads; append, in tie-break order: **Suggestions** (every submenu child flattened, e.g. "New thread in {name}", "Change theme: {name}"; not built when the query is a prefix of the submenu's own name), **Threads**, **Projects**, **Settings** (settings search items, description "Settings · {section}") |
| In a submenu | search only the submenu; if nothing matches, fall back to a root search |
| Normalization | NFKD, strip combining marks, lowercase, curly → straight apostrophes, collapse whitespace. A query in double quotes is an exact phrase |
| Word starts | after a non-alphanumeric, camelCase boundary, last capital of an acronym, digit/letter boundary, every CJK char |
| Token score | word start that ends at a word end 1.0; word-start prefix `0.7 + 0.2 × tokenLen / wordLen`; mid-word 0.3 (tokens ≥ 4 chars, thread titles only); joined words in the first field ("vscode" ↔ "VS Code") 0.6; one typo 0.45 (tokens ≥ 3, not numeric, first letter kept except a swap of the first two when length > 3; tokens ≤ 4 only forgive a swap or one inserted letter) |
| Typo pass | only runs if no item in any group had a strong match (all tokens at word starts, no typo) |
| Filler words | a, an, and, for, in, of, on, the, to, with are optional when the query has other words |
| Field quality | mean over tokens of best `score × fieldWeight`; +0.5 if the first field equals the whole query; +0.25 if multi-word and the first field contains the whole query |
| Commands / projects / settings | title weight 1; keywords 0.8 (query < 4 meaningful chars, whole-word only) or 0.6; keywords only at word starts, no typos; keyword-only matches capped at quality 0.65 for group ranking |
| Threads | weights: title 1 (mid-word allowed), "#PR number" 1, PR title 0.5, branch 0.6 (strip "t3/"), project 0.25, environment 0.25. Score = quality × `min(2.5, 1 + recency + 0.5·log2(1 + opens))`, recency and opens halving every 14 days. A whole-title match is capped at 1.49 for group ranking |
| Match class | 3 if quality ≥ 1.5, 2 if ≥ 0.7, 1 if ≥ 0.4, else 0 |
| Sort within a group | match class (capped at 2) → exact past pick for this query → full match class → most recent pick → whole word → score. Rows flagged secondary go last |
| Sort groups | best match class desc, then insertion order above |
| Limits | Threads 20; Suggestions 3; other groups 3 when threads also matched, else unlimited |
| Highlight | first row highlighted on every query change (except path browse); Enter runs the highlighted row, or the first |

Tern can store pick history locally (the source syncs it to the server for ranking, and the "In messages" server message-search group is extra; keep both optional).

### 4.5 Add project

`Add project` (root action 11, the sidebar's "Add project", the scope popup's "Add project", `NoProjectsHero`) all land here (`CommandPalette.tsx:1102-1208, 1994-2136`, `CommandPaletteAddProject.tsx`, `CommandPalette.logic.ts:67-78, 732-775`).

With one (local) environment the palette pushes a **Sources** group. Tern keeps one row: **Local folder** (`folder-plus`, description "Browse a folder on disk"). ("New project" (`folder-git-2`, "Start a new Git repository from a name") is optional; Git URL and hosted-repo clone are dropped.)

Local folder = in-palette path browser:

| Part | Spec |
|---|---|
| Start | pushes a view whose query is the server's `addProjectBaseDirectory`, else `~/` |
| Trigger | browse mode is on whenever the query starts with `/`, `~/`, `./` or `../`, even at the root |
| Listing | RPC `filesystemEnvironment.browse({partialPath, cwd?})` → `{entries: [{name, fullPath}], parentPath}`. One group "Directories": `..` row (`corner-left-up`) when going up is possible, then one `folder` row per directory |
| Filter | text after the last `/` filters entries: exact, then prefix, then word boundary, then substring, then fuzzy. Dot-folders only when the filter starts with "." |
| Status | "Loading folders..." or the access error, 14 `muted-foreground`, px 16, py 12 |
| Tab | enter the highlighted folder, or complete to the longest common prefix |
| Enter | on a highlighted folder: navigate into it. With nothing highlighted: submit the typed path |
| Ctrl+Enter | submit the typed path even with a row highlighted |
| Submit button | inside the input, right 10, vertically centered, `outline` size `xs`: "Add" (or "Create & Add" if the path doesn't exist) + Kbd "Enter" / "Ctrl Enter". The input's right padding grows to 96 / 120 / 152 to clear it |
| Native picker | footer button "Open in Files" → native folder dialog (`pickFolder({initialPath})`), then submit the chosen path |
| Footer action label | "Add" |

Submit (`handleAddProjectForEnvironment`, `CommandPalette.tsx:1994-2136`):

1. Environment disconnected → error toast "Failed to add project" / "Environment unavailable".
2. Relative path with no active project → "Relative paths require an active project."
3. Path matches an existing project → unarchive it if archived, open its latest thread (or a new draft), close the palette.
4. Otherwise RPC `projectEnvironment.create({projectId: new, title: inferProjectTitleFromPath(cwd) (the folder's basename), workspaceRoot: cwd, createWorkspaceRootIfMissing: true, defaultModelSelection: null})`.
5. Open a new draft thread in the new project and close the palette. No success toast.

Dropping a folder onto the sidebar (`sidebar/SidebarFolderDrop.tsx`) adds it the same way; files dropped there toast "Only folders can be added as projects" / "Drop files on a thread to attach them." (info).

`AddProjectToMachineDialog` is **not** the add-project flow: it clones an existing project onto another connected machine (opened from the branch toolbar). Out of scope.

---

## 5. Default keybindings

Sources: `packages/shared/src/keybindings.ts:58-228` (defaults, key parser), `:230-417` (`when` grammar, merge), `packages/shared/src/keybindingMatching.ts`, `apps/web/src/keybindings.ts` (resolution, labels), `apps/web/src/shortcuts/dispatcher.ts`, `packages/contracts/src/keybindings.ts`, `apps/server/src/keybindings.ts:472-583` (user file sync).

### 5.1 Model

- Key string: lowercase, `+`-separated, exactly one non-modifier key. Modifiers `mod`, `ctrl`/`control`, `shift`, `alt`/`option`, `cmd`/`meta`. `space`, `esc`, `mod++` supported. **`mod` = Ctrl on Linux**. Modifier match is exact (Ctrl+K does not fire with Shift held). No chords.
- Key match candidates: lowercased `key`, the letter from `code` (`KeyX`), and the physical digit/punctuation from `code` (so Ctrl+Shift+1 matches though `key` is "!", and `[` works on non-US layouts). AltGr + non-alphanumeric never matches.
- `when`: identifiers, `!`, `&&`, `||`, parentheses; precedence `!` > `&&` > `||`; `true`/`false` literals; unknown identifiers are false. Unparseable rules are dropped silently.
- Context keys: `terminalFocus`, `previewFocus`, `editableFocus`, `modelPickerOpen` (computed per event); `terminalOpen`, `previewOpen`, `rightPanelOpen`, `isWeb`, `isDesktop`, `isMac`; `composerFocus`, `draftThreadRoute`, `turnRunning`, `composerRequiresModifier`; `usagePageOpen`.
- Resolution: walk the list from the end; the first rule whose `when` passes and whose key matches wins (last wins).
- User file: `~/.t3/userdata/keybindings.json` (`$T3CODE_HOME` overrides `~/.t3`); Tern should use its own path. JSON array of `{key, command, when?}`, max 256. A command named in the user file drops all its defaults; remaining defaults go first, user rules after. Unbind with `"when": "false"`. Missing file → write the defaults. Watched and live-reloaded.
- Labels on Linux: `Ctrl`, `Alt`, `Shift`, `Meta` then key, joined by "+", e.g. `Ctrl+Shift+K`; single chars uppercased; `Esc`, `Space`, `Up/Down/Left/Right`, `F6`, `Enter`. The label shown for a command is its first *effective* binding in the current context (shadowed bindings are never shown).
- Commands are swallowed while focus is inside an alert dialog. Capture-phase scopes skip targets inside `[data-keybinding-capture]` (the settings key recorder).

### 5.2 Defaults (Linux)

| Command | Key | when |
|---|---|---|
| `sidebar.toggle` | Ctrl+B | (code also refuses while `terminalFocus`) |
| `workspace.focusNext` | F6, Ctrl+F6 | |
| `workspace.focusPrevious` | Shift+F6, Ctrl+Shift+F6 | |
| `navigation.back` | Ctrl+[ | `!terminalFocus` |
| `navigation.forward` | Ctrl+] | `!terminalFocus` |
| `terminal.toggle` | Ctrl+J | |
| `rightPanel.toggle` | Ctrl+Alt+B | |
| `threadPanel.toggle` | Ctrl+Shift+B | `!terminalFocus` |
| `rightPanel.new` | Ctrl+T | `rightPanelOpen && !terminalFocus` |
| `rightPanel.close` | Ctrl+W | `!terminalFocus` |
| `terminal.split` | Ctrl+D | `terminalFocus` |
| `terminal.splitVertical` | Ctrl+Shift+D | `terminalFocus` |
| `terminal.new` | Ctrl+N | `terminalFocus` |
| `terminal.close` | Ctrl+W | `terminalFocus` |
| `terminal.previous` / `terminal.next` | Ctrl+Alt+[ / Ctrl+Alt+] | `terminalFocus && !isMac` |
| `view.reopenClosed` | Ctrl+Shift+T | |
| `diff.toggle` | Ctrl+D | `!terminalFocus` |
| `preview.toggle` | Ctrl+Shift+J | |
| `preview.refresh` | Ctrl+R | `previewFocus` |
| `preview.focusUrl` | Ctrl+L | `previewFocus` |
| `preview.zoomIn` | Ctrl+=, Ctrl++ | `previewFocus` |
| `preview.zoomOut` | Ctrl+- | `previewFocus` |
| `preview.resetZoom` | Ctrl+0 | `previewFocus` |
| `preview.find` | Ctrl+F | `previewFocus` |
| `commandPalette.toggle` | Ctrl+K | `!terminalFocus` |
| `filePicker.toggle` | Ctrl+P | `!terminalFocus` |
| `projectSearch.toggle` | Ctrl+Shift+F | `!terminalFocus` |
| `usage.open` | Ctrl+U | `!terminalFocus` |
| `theme.select` | Ctrl+Alt+A | `!terminalFocus` |
| `appearance.cycle` | Ctrl+Alt+Shift+A | `!terminalFocus` |
| `themeEditor.toggle` | Ctrl+Alt+Shift+T | |
| `composer.stash` | Ctrl+S | `!terminalFocus` |
| `thread.steerQueuedMessage` | Ctrl+Shift+Enter | `!terminalFocus` |
| `thread.editQueuedMessage` | Alt+Up | `composerFocus` |
| `composer.sendAlternate` | Ctrl+Enter | `composerFocus && turnRunning` |
| `composer.sendBackground` | Ctrl+Enter, Ctrl+Alt+Enter | `composerFocus && draftThreadRoute` |
| `composer.sendAndNewThread` | Ctrl+Alt+Enter | `composerFocus && !draftThreadRoute` |
| `chat.new` | Ctrl+N, Ctrl+Shift+O | `!terminalFocus` |
| `chat.newLocal` | Ctrl+Shift+N | `!terminalFocus` |
| `chat.newWithoutProject` | Ctrl+Alt+N | `!terminalFocus` |
| `chat.find` | Ctrl+F | `!terminalFocus && !previewFocus` |
| `modelPicker.toggle` | Ctrl+Shift+M | `!terminalFocus` |
| `composer.host` | Ctrl+Shift+H | `!terminalFocus` |
| `composer.effort` | Ctrl+Shift+E | `!terminalFocus` |
| `composer.mode` | Ctrl+Shift+A | `!terminalFocus` |
| `composer.workspace` | Ctrl+Shift+X | `!terminalFocus` |
| `composer.branch` | Ctrl+Shift+G | `!terminalFocus` |
| `composer.previousWorktree` | Ctrl+Shift+L | `!terminalFocus` |
| `pullRequest.copyNumber` | Ctrl+Shift+K | `!terminalFocus` |
| `modelPicker.previousProvider` / `nextProvider` | Ctrl+Shift+Up / Down | `modelPickerOpen` |
| `modelPicker.jump.1…9` | Ctrl+1…9 | `modelPickerOpen && isDesktop` |
| `editor.openFavorite` | Ctrl+O | (ignores key repeat) |
| `thread.previous` | Ctrl+Shift+[ | |
| `thread.next` | Ctrl+Shift+] | |
| `thread.copyReference` | Ctrl+Shift+C | `!terminalFocus` |
| `thread.settle` | Ctrl+Shift+S | `!terminalFocus` |
| `thread.pin` | Ctrl+Shift+P | `!terminalFocus` |
| `thread.undo` | Ctrl+Z | `!terminalFocus && !editableFocus` (editable focus is ignored while an undo notice is live) |
| `thread.jump.1…9` | Ctrl+1…9 | `!modelPickerOpen && isDesktop` |
| `usage.cost` / `usage.tokens` / `usage.limits` | C / T / L | `usagePageOpen` |
| `usage.period.day/week/month/quarter` | Ctrl+Shift+1…4 | `usagePageOpen` |

Also valid, unbound by default: `thread.stop`, `thread.regenerateTitle`, `rightPanel.toggleMaximized`, `composer.cycleHost`, `composer.increaseEffort`, `composer.decreaseEffort`, `script.<id>.run`. Drop `composer.dictation` (Ctrl+Shift+D, `!terminalFocus`): voice is out.

Shell-level non-configurable keys: Escape leaves `/pull-requests` (back, or to `/`) when nothing else claimed it; F2 renames a focused sidebar row; Ctrl+click / Shift+click select rows.

---

## 6. Toasts and confirm dialogs

Sources: `components/ui/toast.tsx`, `ui/toast.logic.ts`, `ui/toastHelpers.ts`, `confirmDialog.ts`, `components/ConfirmDialogHost.tsx`, `ui/alert-dialog.tsx`, `ui/dialog-styles.ts`, `index.css:452-494`.

### 6.1 Toasts

| Part | Spec |
|---|---|
| Position | top-right. Inset 32px (16 below 640). Top = inset + topbar = **72** on Linux (32 + 40). Right = 32. z 100 |
| Width | `100% − 2 × inset`, max **360** |
| Surface | `popover` at 80% + backdrop blur 12, 1px `contrast-foreground/10%`, radius 8, `shadow-xl` black/25, text `popover-foreground` |
| Inline layout (default) | py 12, pl 14, pr 24 with trailing controls else 14; row, centered, space-between, gap 6, text 14 |
| Stacked layout (`stackedThreadToast`) | column, gap 8, py 10, px 14; text block pr 20; actions right-aligned below |
| Text | column gap 2. Title row gap 8: icon cell 16 wide, title `font-medium`, wraps. Description `muted-foreground`, selectable; error descriptions ≥ 180 chars clamp to 4 lines |
| Type icons | error `circle-alert` `destructive`; info `info` `info`; success `circle-check` `success`; warning `triangle-alert` `warning`; loading spinner at 80%; untyped none |
| Close | 24px circle at top −6, right −6 (overhangs the corner), 1px `border/60`, bg `popover/92` + blur, `muted-foreground`, `x` 12 stroke 2.25; hover bg `popover`, text `foreground` |
| Trailing controls (gap 6) | error copy button (`icon-micro` 20, `ghost-muted`, `copy` 12 → `check` `success`, tooltip "Copy error"/"Copied error"); extra actions; secondary (`outline`); primary (`default`). All `xs` (h24) |
| Expandable | "Show details"/"Hide details", 12 medium `muted-foreground`, chevron 14; panel 8 below, max-h 160, scrolls |
| Stack | front toast index 0. Collapsed: toasts behind take the front height, scale `1 − 0.1 × index`, shifted down `index × 12` + `(1 − scale) × height`, their content hidden. Expanded on hover/focus: stacked by real heights with 12px gaps. Base UI defaults (unverified): 3 visible, **5000ms** timeout, pause while expanded/hovered |
| Motion | transform 0.5s `cubic-bezier(.22,1,.36,1)`, opacity 0.5s, height 0.15s. Enter from the right by `100% + inset`; exit fades and slides right; swipe right or up to dismiss |
| Thread-scoped | a toast tagged with a thread shows only while that thread is the open route |
| `timeout: 0` | sticky until dismissed |

Anchored toasts (next to an element, offset 4, max-w min(256, available), text 12; tooltip style = radius 6, px 8, py 4, title only; scale 0.98 + fade): used for "Copied" feedback and "Failed to copy".

Shell toasts to implement (titles; errors carry the thrown message, else "An error occurred."):

| Origin | Toast |
|---|---|
| Rename | warning "Thread title cannot be empty"; error "Failed to rename thread" |
| Thread state | error "Failed to settle thread", "Failed to un-settle thread", "Failed to pin thread", "Failed to unpin thread", "Failed to archive thread", "Thread archived, but navigation failed", "Failed to delete thread", "Failed to delete threads", "Thread deleted, but navigation failed" |
| Copy | error "Failed to copy path/branch/thread ID"; "Path unavailable" / "This thread does not have a workspace path to copy." |
| Multi-select | error "Thread action unavailable" / "This connection cannot change one or more selected threads." |
| Title regen | error "Failed to regenerate thread title"; info "Title kept" / "The current title already fits this thread." |
| Turn into project | success "Project created" (description: thread title); error "Could not turn thread into a project" |
| New thread | error "Could not create thread" |
| Worktree cleanup | error "Failed to delete worktree" / "Could not remove {path}. {message}" |
| Folder drop | info "Only folders can be added as projects" / "Drop files on a thread to attach them."; error 'Could not add "{name}"' |
| Palette | error "Unable to run command", "Failed to add project", "Failed to open project", "Cannot add project", "Environment unavailable"; success "Agent session will restart"; error "Couldn't save theme selection" / "Try again."; `appearance.cycle` shows a 1500ms toast with the new appearance |
| Undo failure | error with the undo's own failure title |

The undo notice for settle/archive/etc. is **not** a toast; it lives in the sidebar footer (2.11).

### 6.2 Confirm dialog

API: `confirm(message, {variant: "default" | "destructive", initialFocus: "confirm" | "cancel"}) → bool`. One message string is split (`ConfirmDialogHost.tsx:166-192`): the title is the first line ending in `?` (or `？`); every other line is the description (newlines kept). No `?` line → title is the text up to the first `?`; no `?` at all → title "Confirm action", description the message. Buttons are always **Cancel** (`outline`) and **Confirm** (`default` or `destructive`). Requests queue one at a time.

| Part | Spec |
|---|---|
| Backdrop | fixed, z 50, `background` 60% (64% dark), blur 4, 200ms fade. Clicking it does not dismiss |
| Placement | 3-row grid (1fr auto 1fr), padding 16, centered |
| Popup | width 100%, **max 512**, radius 16, 1px border, `popover-foreground`, same glass surface and shadow as the palette, scale 0.98 + fade 200ms |
| Header | padding 24, column gap 8, left-aligned. Title 20 semibold, line-height 1.375. Description 14 `muted-foreground`, max-h min(384, 60% window), scrolls |
| Footer | 1px top border, bg `muted/72%`, py 16, px 24, row right-aligned, gap 8, bottom radius 15 |
| Buttons | `default` size: h32, px 11, 14px, radius 8 |
| Keys | Escape = Cancel. Focus starts on Cancel (on Confirm only for non-destructive with `initialFocus: "confirm"`). Enter activates the focused button. App shortcuts are blocked inside |

Shell confirms:

| Action | Message (line 1 = title) | Variant | Gate |
|---|---|---|---|
| Delete thread | `Delete thread "{title}"?` / `This permanently clears conversation history for this thread.` | destructive | `confirmThreadDelete` (default on) |
| Delete N threads | `Delete {n} threads?` / `This permanently clears conversation history for these threads.` | destructive | same |
| Archive thread | `Archive thread "{title}"?` | default | `confirmThreadArchive` (default off) |
| Unpin thread | `Unpin thread "{title}"?` / `This will move the thread out of your pinned section.` | default | `confirmThreadUnpin` |
| Delete orphaned worktree | `This thread is the only one linked to this worktree:` / `{path}` / `` / `Delete the worktree too?` | destructive | after deleting a thread |
| Close terminal | `Close terminal "{label}"?` / `This stops the running process and clears its history.` | destructive | `confirmTerminalClose` |
| Remove project | `Remove project "{label}"?` (or `… and delete its {n} threads?`) / `Path: {path}` / … / `This removes only the project entries, not the files on disk.` / `This action cannot be undone.` | destructive | always |

---

## 7. Skip on purpose

| Feature | Where | Why |
|---|---|---|
| Updater: app update pill, provider update pill, "Update everything" dialog, architecture warning, install-update confirm | `sidebar/SidebarUpdatePill.tsx`, `SidebarProviderUpdatePill.tsx`, `UpdateEverythingDialog.tsx`, `updateEverythingRun.tsx`, `DesktopUpdateStatusIcon.tsx` | updater dropped |
| Environment stage backdrop and environment pill in the sidebar header | `SidebarStageBackdrop.tsx`, `SidebarChrome.tsx:58-94` | dev/staging and remote environment identification |
| Remote environments: machine icons, environment labels on rows, environment badges in project lists, Environments group in add-project, "Add to another machine" dialog, SSH password prompt | many | remote devices/pairing dropped. Keep the data model's single local environment |
| Guest sharing: "Share with guest", the **Request** status, guest share dialog | `threadActionMenu.logic.ts`, `Sidebar.logic.ts:978-981` | dropped |
| Cloud/Connect: connect onboarding, relay client install, device approval coordinator, hosted-static/hosted pairing branches, `/pair`, `/connect*`, `/guest` routes | `routes/__root.tsx:165-212` | dropped |
| Port forwarding palette submenu | `CommandPalettePortForwardItems.tsx` | remote |
| Prompt links: "Copy prompt link", `/new` deep links, hosted prompt-link landing | `routes/_chat.new.tsx`, palette | hosted app URLs |
| "Use T3 Code in another app (MCP)", "Open workflows", "Triage an issue", Snap Shot, ChatGPT welcome | palette, `__root.tsx` | MCP/workflows/marketing-ish agent flows |
| Voice: global voice indicator, `composer.dictation` | `_chat.tsx:28` | dropped |
| **Snooze**: snooze button and presets, custom snooze dialog, Snoozed shelf, Woke status, wake timers, `alarm-clock` badges | `Sidebar.snooze.ts`, `CustomSnoozeDialog.tsx`, rows | slop: a second triage axis on top of settle/pin with its own clock and server state. Settle covers the need |
| Action sweep (drag across row buttons to settle many) | `Sidebar.tsx` `startActionSweep` | slop; multi-select + context menu does the same |
| Auto-settle behaviour submenu | context menu | keep server auto-settle if the server does it; drop the per-thread opt-out UI |
| History swipe navigation, macOS traffic-light/fullscreen handling | `HistorySwipeNavigation.tsx`, `AppSidebarLayout.tsx:82-117` | macOS trackpad only |
| Mobile sheet sidebar below 768px | `ui/sidebar.tsx:332-380` | the window can be that narrow (min 400) so a fallback is needed, but it can be a plain overlay; not worth pixel-matching |
| Pull requests / Issues footer buttons and pages | `SidebarChrome.tsx:289-302` | depends on hosted source control; show only if Tern implements those pages |
| "In messages" server-side message search, server-synced pick history | palette | optional; ranking works without them |
| Dev replay host, theme editor host | `__root.tsx` | dev-only / separate settings work |
| Intent prefetch, row subscription leases, content-visibility hints | `useThreadIntentPrefetch.ts`, `Sidebar.logic.ts:92-123` | web performance tricks, not behaviour |
