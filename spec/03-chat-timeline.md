# 03 · Chat header and timeline

Source of truth: `forks/t3code/apps/web/src`. Paths below are relative to that directory
unless they start with `packages/`. `chat/` means `components/chat/`.

Conventions in this file:

- px values assume the default interface font size of 16px. The app sets the root font size
  from Settings > Appearance (12 to 20px, `appearanceFonts.ts:100`), so every rem value
  scales with it. Code font size is separate, default 13px (`--font-size-code`), prompt 14px.
- `sm` = window width ≥ 640px. "a / b" means a below 640px, b at 640px and up.
- Colors are token names (`muted-foreground`, `border/70` means `border` at 70% alpha).
- Icons are Lucide names (`chevron-down`). Sizes: `size-3` 12px, `size-3.5` 14px, `size-4` 16px.
- Radii: `rounded-sm` 4, `rounded-md` 6, `rounded-lg` 8, `rounded-xl` 12, `rounded-2xl` 16px.
- Type: `text-3xs` 10/14px, `text-2xs` 11/16px, `text-xs` 12/16px, `text-sm` 14/20px.
  `leading-relaxed` = 1.625 (14px → 22.75px line).
- UI strings are verbatim, in double quotes. `{x}` is an interpolation.
- Button sizes used below (`components/ui/button.tsx:13-45`, radius 8px unless noted):

  | Size | Height | Padding-x | Text | Icon |
  |---|---|---|---|---|
  | `xs` | 28 / 24px | 7px | 14 / 12px | 16 / 14px |
  | `icon-xs` | 28×28 / 24×24 | 0 | | 16 / 14px |
  | `sm` | 32 / 28px | 9px | 16 / 14px | 18 / 16px |
  | `micro` | 20px, radius 4px | 5px | 11px | 12px |

  Variants: `ghost` (transparent, hover bg `accent`, icon `contrast-muted-foreground`),
  `ghost-muted` (text `muted-foreground`, hover bg `accent` + `foreground`), `outline` (1px
  `input` border, bg `popover`, dark bg `input/32`, hover `accent/50`), `secondary` (bg
  `secondary`), `default` (bg `primary`, text `primary-foreground`), `glass` (rounded-full,
  translucent `background`, 1px `border/60`). Buttons scale to 97% while pressed. Icon sizes
  set explicitly in this spec override the table.
- `InlineButton` is link-style text: medium weight, underline on hover, no padding, tone
  `default` (`foreground`) or `muted` (`muted-foreground` → `foreground`).

This fork is far ahead of upstream T3 Code: it runs on "orchestration v2" turn items, runs,
attempts and subagents. The spec follows the fork.

---

## 1. Chat header

`ChatView.tsx:3566-3608`, `chat/ChatHeader.tsx:253-418`, `components/WorkspaceBreadcrumb.tsx`,
`chat/PanelLayoutControls.tsx`.

### 1.1 Bar

| Property | Value |
|---|---|
| Height | 52px (`--workspace-topbar-height`, `index.css:123`) |
| Background | `background` |
| Padding-x | 12 / 20px |
| Padding-left when sidebar is collapsed | 52px (`--workspace-titlebar-content-left` = 12 + 28 + 12, `ui/sidebar.tsx:249`) so the title clears the sidebar toggle |
| Window drag | whole bar drags the window; buttons/inputs inside are no-drag |
| Layout | one row, `items-center` |

Inside the bar, `ChatHeader` is `flex min-w-0 flex-1 items-center`, gap 8 / 12px, padding-right
96px (`pr-24`) to leave room for the fixed titlebar controls (§1.3). When an inline right panel
owns the title bar it is 40px (`pr-10`) (`ChatHeader.tsx:256-259`).

### 1.2 Breadcrumb (left)

`<nav aria-label="Thread breadcrumb">` → `<ol>` flex, gap 8 / 12px, `text-sm`, `font-medium`
(`WorkspaceBreadcrumb.tsx:11-17`). Each text span truncates with ellipsis and is trimmed to cap
height with 6px vertical padding (`WorkspaceBreadcrumb.tsx:27-38`).

Order: `[project] / [parent thread] / [title] [badge]`.

| Crumb | Shown when | Look | Click | Tooltip (top) |
|---|---|---|---|---|
| Project | thread has a project | project favicon 14px + name, gap 6px, middle-truncated, `muted-foreground` → `foreground` on hover. Scratch project name: "No project" | new thread in this project | "New thread in {project}" |
| Separator | after each non-last crumb | "/" in `icon-muted` | | |
| Parent thread | thread is a subagent or side chat | title, `muted-foreground` → `foreground` hover. When the whole row overflows, the title collapses to width 0 and "..." shows instead (measured, `ChatHeader.tsx:124-162`) | opens parent thread | parent title |
| Title (current) | always | `<h2>`, `foreground`, truncate, min-width 40px, flex-1 | opens thread action menu anchored at (left, bottom + 4px) | full title |
| Title chevron | server thread | `chevron-down` 14px, `muted-foreground`, opacity 0, 1 on hover or focus | (part of title button) | |

Title interactions (`ChatHeader.tsx:203-252`):

- Double-click title (no modifier keys, not on the chevron): close menu, start inline rename.
- Right-click anywhere in the header: thread action menu at the cursor. For a local draft (not
  yet on the server) the menu has one item "Project settings".
- The thread action menu is shared with the sidebar (`hooks/useThreadActionMenu.ts`); its items
  belong to the sidebar spec.

Inline rename input (`ChatHeader.tsx:337-357`): replaces the title in place. `text-sm`
`font-medium` `foreground`, transparent background, radius 4px, 1px inset ring `ring/50`,
`ring` when focused. Text is selected on focus.

| Key/event | Effect |
|---|---|
| Enter | commit |
| Escape | cancel |
| Blur | commit, unless focus moved to a button (then cancel) |

Commit rules: trim; empty → warning toast "Thread title cannot be empty"; unchanged → nothing;
else send `thread.updateMetadata { threadId, title }`. On failure: error toast
"Failed to rename thread" with the error message or "An error occurred.".

Skip on purpose: the "From {origin}" info badge (linked environment, T3 Connect) and
`GuestShareIndicator` (guest sharing).

### 1.3 Right-side controls (fixed titlebar controls)

`PanelLayoutControls.tsx:46-152, 200-268`. Rendered `position: fixed`, top 0, right 12px,
height 52px, `items-center`, gap 4px, z-index 50, no-drag. Not part of the header's flex flow.

Order left to right:

| # | Control | Icon (16px) | Shown | Tooltip (bottom) | Action |
|---|---|---|---|---|---|
| 1 | Maximize panel | `maximize-2` / `minimize-2` when maximized | only while the right panel is open; fades in | "Maximize panel" / "Restore panel size" | toggle right panel maximized (chat column collapses to width 0) |
| 2 | Thread details | `square-menu` | always | "Toggle thread details ({shortcut})" or "Toggle thread details" | toggle thread details panel (or popover when presented as popover) |
| 3 | Terminal drawer | `panel-bottom` | always; disabled without a project | "Toggle terminal drawer ({shortcut})" / "Toggle terminal drawer" / "Terminal drawer is unavailable" | toggle bottom terminal |
| 4 | Right panel | `panel-right` | always; disabled without a project | "Toggle right panel ({shortcut})" / "Toggle right panel" / "Right panel is unavailable" | toggle right panel |

Each is a Toggle, variant ghost, size sm (`ui/toggle.tsx:14-35`):

| Property | Value |
|---|---|
| Size | 32×32 / 28×28 |
| Radius | 8px |
| Border | transparent |
| Icon | 16px at 80% opacity |
| Hover | bg `accent` |
| Pressed (on) | bg `accent`, text `accent-foreground` |
| Disabled | text `muted-foreground`, no pointer |
| Focus | 2px inset ring `ring` |

When the right panel opens inline it covers the right side of the header; then only the thread
details toggle remains in the header at right 12px (`ThreadPanelHeaderControl`,
`PanelLayoutControls.tsx:186-198`), and the other toggles move into the panel. Closing the panel
slides the toggle back with the panel's own transition.

### 1.4 Not in the header in this fork

The open-in-editor picker, branch, git actions and changes/diff entry point are not header
controls in this fork. They live in the thread details panel (`chat/ThreadDetailsPanel.tsx:206-290`:
`BranchToolbar`, `OpenInPicker displayMode="panel"`, `ProjectScriptsControl`,
`GitActionsControl`). Diffs open as a right-panel surface. Build them with the thread details
panel spec, not here.

---

## 2. Timeline layout

### 2.1 Column structure

`ChatView.tsx:3536-3770`, `chat/ChatCanvas.tsx:170-200`, `index.css:979-988, 2660-2697`.

```
chat column (flex col, bg background)
├─ header (52px, §1)
└─ canvas (relative, flex-1)
   ├─ ThreadFind bar (absolute, §6.1)
   ├─ banner lane (absolute top, z 30, §3.12)
   ├─ conversation area (relative, flex-1)
   │  ├─ timeline scroll view (virtualized list, §2.3)
   │  ├─ minimap (absolute left, §6.2)
   │  └─ "Scroll to end" pill (absolute, §5.2)
   └─ composer overlay (absolute, bottom 0, z 20, padding-top 6 / 8px), see composer spec
```

The timeline scrolls underneath the composer overlay. The list footer reserves the overlay's
measured height so the last row can scroll clear of it (§2.3).

### 2.2 Horizontal geometry

| Item | Value |
|---|---|
| Content lane max width | 736px (`--chat-content-max-width: 46rem`). Setting "wide": 1152px. "full": 100% (`index.css:131, 2378-2386`) |
| Lane | centered (`margin-inline: auto`), width 100%, min-width 0 |
| Scroll view padding-inline | 12 / 48px from each edge of the column, measured to the content (the scroll view reserves a 6px scrollbar gutter on both sides and subtracts it, `index.css:2667-2694`) |
| Composer lane | same 12 / 48px inset, same 736px max width, so the composer and the messages share edges |
| Lane insets | `--chat-lane-inset-start/end` are 0 unless a floating preview or inline details card overlaps the chat (ChatCanvas). Tern can treat them as 0 |
| Scrollbar | 6px wide, radius 3px, transparent track; thumb `--app-scrollbar-thumb` (light rgb(217 217 217), dark white 36%, hover 45%) |
| Horizontal overflow | clipped per row (`overflow-x: clip`) |

### 2.3 Vertical geometry of the list

`chat/MessagesTimeline.tsx:166-205, 1232-1278, 1496-1562`.

| Part | Height |
|---|---|
| List header | 24px when the top fade is on (default; off while a banner is pinned at the top), else 12 / 16px. When the thread has a parent, the header is a "Subagent of" / "Context from" divider instead (§3.11) |
| "Load earlier turns" control | optional, under the header (§3.13) |
| Rows | see §2.4 |
| Footer content | queued turns, lifecycle status line, unmatched pending approvals (§3.8) |
| Footer spacer | composer overlay height (measured) |
| Footer tail | 12 / 16px |

Top fade (`topbar-scroll-fade`, `index.css:496-530`): the top 25px of the scroll viewport is
masked from transparent to opaque with stops 0% → 10% → 30% → 58% → 82% → 96% → 100% at
0/10/24/42/62/82/100% of the band. The scrollbar strip on the right is not masked.

Empty thread: centered "Send a message to start the conversation." in `text-sm`
`muted-foreground`, bottom padding = composer height (`MessagesTimeline.tsx:1465-1476`).

### 2.4 Vertical rhythm between rows

Each row is wrapped in a lane-width frame. Spacing is padding-bottom on the row
(`MessagesTimeline.tsx:1667-1700`, `chat/WorkLog.tsx:15-37`):

| Row | Padding-bottom |
|---|---|
| Work log rows (`work`, `work-live`, `work-toggle`, `thinking`) | 8px; 0 when the next row is also a work log row (`continuesWorkLog`) or the row is an expanded group header; 4px for expanded group content |
| `turn-fold`, `working` | 6px |
| User message followed by another of your own messages | 4px |
| Assistant message without meta, `worktree-setup`, `event`, `attempt-fold`, `sent-message`, `viewed-images` | 8px |
| Everything else (user message, assistant message with meta, `assistant-meta`, `proposed-plan`, `context-compaction`) | 16px |

Subagent group events use the work log spacing.

### 2.5 Data model the rows come from

Thread data arrives as ordered "visible turn items" (`OrchestrationV2ProjectedTurnItem`).
`session-logic.ts:559-795` turns them into `TimelineEntry` values, then
`chat/MessagesTimeline.logic.ts:1619-2448` turns entries into display rows.

**Turn item → timeline entry** (`session-logic.ts:620-760`):

| Turn item type | Entry kind |
|---|---|
| `user_message`, `assistant_message` | `message` (role user / assistant; assistant carries `streaming`) |
| `proposed_plan` | `proposed-plan` |
| `dynamic_tool` completed that returns an HTML render | `html-render` (skip, §7) |
| `dynamic_tool` completed that is a `send_to_user` | `sent-message` |
| `fork`, `handoff`, `run_interrupt_request`, `run_interrupt_result`, `secret_request`, `feedback_request`, `link_request`, `subagent` | `event` |
| `checkpoint` | dropped |
| everything else (`reasoning`, `command_execution`, `file_change`, `file_search`, `web_search`, `dynamic_tool`, `todo_list`, `approval_request`, `user_input_request`, `system_notice`, `error`, `compaction`, `thread_created`, `notification`) | `work` with a `WorkLogEntry` |

Optimistic (just sent, not yet on the server) user messages are appended at the end; queued
turns are not timeline rows (they sit in the footer).

**What a work entry is** (`session-logic.ts:372-537`). A `WorkLogEntry` has `label`, optional
`detail`, `command`, `changedFiles`, `tone`, `itemType`, `toolLifecycleStatus`, and the raw item.

| Item | `label` (verbatim when no provider title) | `detail` | tone |
|---|---|---|---|
| `reasoning` | title or "Thinking" | reasoning text | thinking |
| `command_execution` | title or "Ran command" | | tool (`command` = input) |
| `file_change` | title, or "Changed {count} files" when >1 change, else "Changed {fileName}" | | tool (`changedFiles`) |
| `file_search` | title or "Searched {query} in {target}" etc. (§2.7) or "Searched files" | pattern | tool |
| `web_search` | title or "Searched the web" | patterns joined ", " | tool |
| `dynamic_tool` | title, else "Read {path}" for read tools, search label for search tools, else tool name, else "Tool call" | | tool |
| `todo_list` | "Tasks {done}/{total} · {current}" or "Tasks {done}/{total}" | | info |
| `approval_request` | title or "Approval requested" | prompt | tool |
| `user_input_request` | title, or "Answered questions" when answered, else "Input requested" | | tool |
| `system_notice` | the message | | info (warning-styled unless level info) |
| `error` | "Usage limit reached" / title / "Provider error"; retries: "Retrying provider ({progress})", "Provider recovered ({progress} retries)", "Provider error after {progress} retries", "Usage limit reached after {progress} retries", "Provider retry stopped ({progress})" | failure message, plus " Retrying in {delay}s." | info |
| `compaction` | compaction label | summary | info |
| `thread_created` | "Created thread" | | tool |

Status mapping (`session-logic.ts:272-290`): pending/running/waiting → `inProgress`;
completed → `completed`; idle → `idle`; failed → `failed`; cancelled/interrupted → `stopped`.

"Tool-like" = tone tool/thinking/error, or has a command, or has a request kind
(`session-logic.ts:162-170`). A tool-like entry has three states:

- failed: tone error, status failed/declined, a command with a non-zero exit code, or output
  matching the failure heuristic in `packages/shared/src/toolOutput.ts:294-305` ("command not
  found", "no such file", "exit code N" and similar, only together with a failure hint).
- success: completed and not failed.
- neutral: anything else (in progress, stopped, idle).

### 2.6 Row derivation

`MessagesTimeline.logic.ts:1619-2448`. Input: entries, `isWorking`, latest run, running run id,
pending approval ids, the three expand sets (`expandedRunIds`, `expandedWorkGroupIds`,
`expandedAttemptIds`, client state, remembered per thread), tool call visibility setting.

**Row kinds** (`MessagesTimeline.logic.ts:530-693`):

| Kind | Meaning |
|---|---|
| `message` | user or assistant message |
| `assistant-meta` | the assistant's meta line moved below trailing tool rows |
| `work` | one or more work entries shown flat; or, with `isExpandedToolGroup`, the contents of an opened group |
| `work-toggle` | a collapsed group of finished work, one summary line |
| `work-live` | the live activity line of the running turn |
| `thinking` | "Thinking" placeholder at the end of a running turn |
| `working` | "Working for 12s" header under the prompt of a running turn |
| `turn-fold` | "Worked for 1m 3s" header of a finished turn that hides its work |
| `attempt-fold` | "Superseded attempt" fold |
| `context-compaction` | compaction divider |
| `viewed-images` | thumbnails of images the agent opened in the group above |
| `proposed-plan`, `sent-message`, `event`, `worktree-setup` | as named |
| `html-render`, `mcp-app` | skip (§7) |

**Pre-filter by tool call visibility** (`MessagesTimeline.logic.ts:1538-1584`), setting
`toolCallVisibility`, default `showAll`:

| Value | Effect |
|---|---|
| `showAll` | nothing hidden |
| `hideSuccessful` | hide completed, non-failed `command_execution`, `file_change`, `file_search`, `web_search`, `dynamic_tool` entries |
| `hideAll` | hide all of those item types |
| `zen` | only user messages and the final assistant message of each finished turn (plus pending approvals and a few cards); no turn folds |

**Grouping rules for work entries** (`MessagesTimeline.logic.ts:2062-2210`). Walk entries in
order. A `work` entry renders alone as a `work` row (no grouping) when any of these hold:

- it is a pending approval
- its run failed and was itemized
- `itemType` is `error` and failed, or tone is error, or it is a runtime error
- `itemType` is `system_notice`, `notification` or `todo_list`
- it carries a question answer

Otherwise it starts a group and absorbs following `work` entries until one of:

- a non-`work` entry
- a compaction, any of the "alone" conditions above
- an entry already shown by the live activity row
- an entry hidden by a fold, or a fold anchor
- a different `runId`, or a different attempt id

Then filter the group to visible entries (`workEntryIsVisibleInGroup`,
`MessagesTimeline.logic.ts:302-313`): reasoning shows only with non-empty text; other entries
show if stopped, or not neutral (so finished successes and failures show; in-progress shows
only inside the active run's expanded group).

The visible group becomes:

| Condition | Rows |
|---|---|
| contains an in-progress entry of the active run | `work-live` (latest in-progress entry), then `work` (expanded contents) if open |
| exactly one entry, and it is tool-like | `work` with `displayLabel` (single line, §3.3). For an edit the label is the group summary ("Changed 1 file") |
| otherwise | `work-toggle` with summary (§2.7), then `work` (expanded contents) if `work-group:{firstEntryId}` is in `expandedWorkGroupIds` |

After any of these, a `viewed-images` row if the group opened workspace images.

**Live activity at the tail of a running turn** (`MessagesTimeline.logic.ts:1746-1813`). While
working, walk back from the last entry to the last prompt and collect trailing `work` entries
of the active run and one attempt, stopping at errors, notices, todos, answers, pending
approvals, compactions or folds. If any are visible and the latest is not a failure, they render
as one `work-live` row at the position of the latest entry:

- `entry` = latest in-progress entry, else latest visible entry.
- `active` = something is in progress, or the latest finished successfully with no approval
  pending. Active rows animate (§3.4).
- `thought` = latest reasoning entry with text; its first line shows above the status line.

If the latest tail entry failed, the tail instead renders as a `thinking` row that toggles the
group.

**Working header** (`MessagesTimeline.logic.ts:1885-1912, 1945-1948, 2381-2386`). While working,
a `working` row is inserted right after the last prompt (`activeTurnHeaderIndex`), so it sits
between your message and the response. `createdAt` = turn start, so it can count up.

**Thinking placeholder** (`MessagesTimeline.logic.ts:2388-2430`). Appended at the end while
working, the run has not ended, the reply is not streaming, no compaction is running, and there
is no active live row and no pending approval. Label: "Waiting on a subagent" / "Waiting on
{count} subagents" when the run's tail is live subagents, else default (§3.5).

**Turn folds** (`MessagesTimeline.logic.ts:1073-1313`). Every finished turn collapses its work
behind one `turn-fold` row placed right after its prompt. Grouping: a turn is everything from a
prompt (a non-steer user message, or a notification) up to the next prompt, keyed by run id.

A turn does not fold when any of these hold:

- it is the active response
- it failed
- it was interrupted
- it still has a streaming message
- folding would hide nothing but compactions

Hidden members: everything except

- the terminal assistant message (the run's last assistant message)
- the latest compaction
- `fork` / `thread_created` cards
- live subagents
- retained assistant replies, notifications, question answers
- entries after the terminal message that are in progress or failed

Label:

| Case | String |
|---|---|
| finished | "Worked for {duration}" (or "Worked" if no times) |
| latest run interrupted by you | "You stopped after {duration}" / "You stopped this response" |
| work after a steer, folded separately under the steer | "Continued for {duration}" / "Continued" |

Duration = run startedAt → completedAt for the latest run, else first entry → max(terminal
message updatedAt, last entry). Format (`packages/shared/src/orchestrationTimingCopy.ts:5-26`):

| Elapsed | Format |
|---|---|
| < 1s | "{ms}ms" (min 1) |
| < 10s | one decimal, "4.2s" |
| < 60s | "{s}s" |
| < 1h | "{m}m {s}s" |
| ≥ 1h | "{h}h {m}m {s}s", "{h}h {s}s" or "{h}h" |

Clicking the fold adds the run id to `expandedRunIds`, which unhides its members in place below
the fold row.

**Superseded attempts** (`MessagesTimeline.logic.ts:847-900, 1983-2006`). Work from a run attempt
that a steer cut off folds behind an `attempt-fold` row, label "Superseded attempt".

**Compaction** (`MessagesTimeline.logic.ts:2012-2050, 2311-2339`). A compaction work entry
becomes a `context-compaction` row. A bare `/compact` command message becomes one too, labelled
"Compacting context" while running, "Context compaction" otherwise.

**Assistant meta placement** (`MessagesTimeline.logic.ts:1315-1407`). The terminal assistant
message of a run shows its meta line (copy, timestamp). If tool rows of the same run follow it
(after a steer, for example), the meta line moves into an `assistant-meta` row after the last of
them, and that line is always visible.

**Post-pass** (`MessagesTimeline.logic.ts:2433-2448`): mark `continuesWorkLog` on a work log row
followed by another; mark consecutive own user messages `groupedWithNext` / `groupedWithPrevious`
(tightens spacing and corners) and `continuesSteer` (steer label shows once).

**Incremental updates** (`chat/useTimelineRows.ts`). Streaming text patches only the streaming
message row; unchanged rows keep identity so the virtual list does not remeasure them. Tern
should key rows by `row.id` and reuse measured heights.

### 2.7 Tool name → label and icon

**Group action** of an entry (`packages/client-runtime/src/work-log/presentation.ts:417-467`,
`packages/shared/src/toolActivity.ts:283-337`), first match wins:

| Rule | Action |
|---|---|
| `thread_created` | `thread-create` |
| request kind `file-read`, or viewed an image | `read` |
| `command_execution`, request kind `command`, data kind `execute`, tool name `terminal`/`bash`/`shell`, or has a command | `command` |
| `file_change`, request kind `file-change`, kind `edit`/`move`/`delete`/`write`, or has changed files | `edit` |
| kind `search`, or tool name `find`/`grep`/`glob`/`rg`/`ls`, or `file_search` | `code-search` |
| `web_search` whose title has no "grep" | `search` |
| kind `read`, tool name `read`/`readfile`, `image_view` | `read` |
| approval/user input request, not tool-like | `update` |
| anything else tool-like | `other` |

**Group summary sentence** (`presentation.ts:552-651, 722-822`). Count entries per action
(edit counts distinct files). Keep the two highest priority clauses (command and edit first,
then read/search/browser, then other/update; ties keep order of first appearance), shown in
order of first appearance. Lowercase every clause after the first. Join: "{a} and {b}", or
"{a}, {b}, and {c}". If more entries remain, add "Performed {count} other action(s)".

| Action | Clause (singular / plural) |
|---|---|
| read | "Read {count} file" / "Read {count} files" |
| edit | "Changed {count} file" / "Changed {count} files" |
| command | "Ran {count} command" / "Ran {count} commands" |
| code-search | "Searched code {count} time" / "… times" |
| search | "Searched the web {count} time" / "… times" |
| browser | "Used browser {count} time" / "… times" |
| device | "Used device controls {count} time" / "… times" |
| thread-create | "Created {count} thread" / "… threads" |
| other | "Used {count} tool" / "Used {count} tools" |
| update | "Received {count} update" / "… updates" |
| only reasoning | "Thought" / "Thought (×{count})" |

Example: "Ran 3 commands and changed 2 files".

**Summary icon** (`chat/TimelineWorkEntryRow.logic.ts:14-55`). If every entry has one action, use
it; else "mixed".

| Summary kind | Icon |
|---|---|
| read | `eye` |
| edit | `square-pen` |
| command | `terminal` |
| code-search | `search` |
| search (web) | `globe` |
| browser | custom browser glyph (`TimelineToolActivityIcon.tsx:59-77`) |
| device | `smartphone` |
| thread-create | T3 wordmark (Tern: app logo) |
| reasoning | `brain` |
| subagent / agent tool | `bot` |
| dynamic tool, update, mixed | `hammer` |
| other | `wrench` |
| tone-only tool | `zap` |

**Single entry icon** (`TimelineWorkEntryRow.logic.ts:154-204`), first match:

| Rule | Icon |
|---|---|
| notification failed | `circle-alert` |
| notification from subagent / delegated task / command / monitor / background task | `bot` / `bot` / `terminal` / `eye` / `zap` |
| `system_notice` info / warning | `zap` / `circle-alert` |
| user input or approval request | `message-circle` |
| `todo_list` | `list-todo` |
| JavaScript REPL tool | `code-2` |
| action ≠ other | summary icon of the action |
| `dynamic_tool` | `wrench` |
| `subagent` | `bot` |
| tone error / thinking / info / tool | `circle-alert` / `brain` / `check` / `zap` |

**Single entry label** (`MessagesTimeline.logic.ts:107-211`), first match:

1. reasoning: the reasoning text collapsed to one line, or "Thought".
2. command: the command with its shell wrapper removed (`bash -lc '…'` shows the inner script) and
   control characters drawn as Unicode control pictures (`commandLabel.ts:1365-1373`). A
   command item without a command: "Ran command" (statuses: "Running command",
   "Failed command", "Declined command", "Stopped command").
3. search: "Searched {query} in {target}", "Searched files {glob} in {target}",
   "Searched files {glob}", "Searched {query}", "Searched in {target}" (query keys: pattern,
   query, searchTerm, regex, grep, needle; glob keys: glob, globPattern, include, filePattern;
   target = last path segment of path/cwd/directory, `toolActivity.ts:340-400`).
4. read: "Read {path}" or "Read {path} +{count} more", workspace-relative; "Read file" if no path.
5. dynamic tool: "Computer use", "Run JavaScript", "Skill: {name}", or the tool title.
6. detail text, unless it is multi-line search output.
7. first changed file, workspace-relative, "+{count} more".
8. tool title or label with a trailing "complete"/"completed" removed, first letter uppercased.

Live label (`work-live`, `MessagesTimeline.logic.ts:259-300`): commands read
"Running {program}", "Ran {program}", "Failed {program}", "Declined {program}",
"Stopped {program}" where program is the parsed program name (`git`, `pnpm`); reasoning reads
its first line or "Thinking" / "Thought".

T3's own MCP tools (thread, schedule, queue, project tools) have their own sentences and icons
(`packages/client-runtime/src/work-log/t3ToolSentences.ts`). Tern has no T3 MCP server; fall
back to the generic rules.

---

## 3. Row visuals

### 3.1 User message

`chat/TimelineUserMessageRow.tsx:460-1137`.

Layout: column, `items-end` (right aligned), gap 4px.

| Part | Spec |
|---|---|
| Steer marker (above bubble, once per run of steers) | `redo-2` 12px + "Steer", or "Steer · waiting to be read" while unread; `text-xs`, leading 1, `muted-foreground`, margin-end 4px. Tooltip: "Steered the active turn" / "Originally queued, then promoted to steer the active turn" / "Waiting for the agent to read this. It reads steers between steps, after a running command finishes." |
| "Sent by T3 Code" (automation-sent) | `text-2xs`, `muted-foreground/70`, margin-end 4px. Tern: "Sent by Tern" |
| Bubble | max-width 80% of lane, radius 16px, bg `message`, text `message-foreground`, padding 12px, min-width 0 |
| Grouped corners | top-right radius 6px if a media block sits above or an own message precedes; bottom-right 6px if an own message follows |
| Body | markdown (§4) with `user-message-markdown` variant: soft line breaks kept, raw HTML not parsed, leading spaces preserved, ordered lists always decimal, inline code tinted with `message-foreground` 10% |
| Long body | if the body is ≥ 352px tall (2 × 176px) it clips to 176px with a 28px bottom fade (mask: black → transparent over the last 1.75rem). Footer, margin-top 6px, right-aligned: ghost-muted xs button "Show full message" / "Show less" (`TimelineUserMessageRow.tsx:1215-1283`, `chat/useTallEnoughToFold.ts`) |
| Status chip under bubble (failed send) | rounded-full, border `destructive/25`, bg `destructive/8`, padding 6×2px, `text-3xs` medium `destructive` |
| Actions row | below bubble, max-width 80%, `justify-end`, padding-end 4px, `text-xs`. Opacity 0, 1 on row hover or focus-within, 200ms fade |

Actions row contents, left to right, gap 8px between timestamp and buttons, 2px between buttons:

| Item | Spec | Action |
|---|---|---|
| Timestamp | `text-xs` `muted-foreground` tabular. "14:05" today, "yesterday at 14:05", else numeric date + time. Tooltip: "14:05, 3rd November 2026" | |
| Edit from here | ghost xs button (28 / 24px tall), `pencil` 12px. Tooltip and label "Edit from here". Shown only if the provider supports rollback and a checkpoint exists for that turn. Disabled while working or reverting | reverts the thread to before this turn and puts this message back into the composer (`chat/useTimelineRewind.ts`). Errors: "This provider does not support reverting conversation history. Start a new thread instead.", "Interrupt the current turn before reverting checkpoints." |
| Copy | ghost-muted xs, `copy` 12px → `check` 12px in `primary` for 1s, disabled while showing check. Label "Copy message" | copies the plain text |
| Copy prompt link | `link` icon, "Copy prompt link" | skip (cloud) |

When the message is grouped with the next one and the lane is ≥ 592px wide, the actions float to
the left of the bubble instead (absolute, end-full, vertically centered, margin-end 8px).

Attachments (`TimelineUserMessageRow.tsx:724-1110`):

| Case | Layout |
|---|---|
| Images with text | 2-column grid, max-width 210px, gap 8px, margin-bottom 8px; cells 4:3, radius 8px, 1px `border/80`, cover |
| Images only, one | sits outside a bubble, radius 16px, height 192px, auto width (min 96px), contain; 1px inset ring black/9 (dark white/12) |
| Images only, collage (2+) | width 320px, gap 1px, radius 16px; 2 images: 2 columns × 192px rows; 3: columns 3fr 2fr, first spans 2 rows of 96px; 4+: 2 columns × 96px, odd last spans 2 |
| Image click | opens the image viewer (gallery of the message's images) |
| Image without preview | name centered, `text-2xs` `muted-foreground/70`, min-height 72px |
| Files (chips) | file icon by extension + name (truncate) + size or "Sending…" in `text-xs` muted; button opens preview, a ghost-muted icon-xs button with `download` icon, tooltip "Download {label}" |
| Files only | each a 256px card: radius 16px, bg `message`, padding 8px, 36px icon tile (radius 8px, bg `background/60`), name `text-sm` medium, size `text-xs` muted |

### 3.2 Assistant message

`chat/TimelineAssistantRows.tsx:28-91, 176-317`.

| Part | Spec |
|---|---|
| Container | no bubble; padding 4px horizontal, 2px vertical; full lane width |
| Body | markdown (§4). Soft line breaks kept only when the text looks like it needs them (`shouldPreserveAssistantLineBreaks`) |
| Empty | "(empty response)", unless streaming or the run produced an embedded result |
| Interrupted, no meta | badge "interrupted" (see below), margin-top 6px |
| Streaming | no cursor. Each newly completed markdown block fades in, opacity 0 → 1 over 600ms ease-out, only while streaming (`index.css:2283-2296`). Only the last block re-renders while streaming (`ChatMarkdown.tsx:2084-2088`) |

Meta line (terminal message of a finished turn), margin-top 6px, `text-xs`, gap 8px, opacity 0,
1 on row hover or focus-within (200ms); always visible when it is a separate `assistant-meta`
row (that row: padding-x 4px, margin-top 2px):

| Item | Spec |
|---|---|
| Status badge | when status ≠ completed: rounded-full, 1px `border/70`, padding 6×2px, mono `text-3xs`, `muted-foreground`, text = raw status ("interrupted", "failed") |
| Copy | as user copy. Hidden while streaming or when text is blank |
| Fork | ghost xs, `git-fork` 12px, "Fork from this response". Only when the provider supports fork. Pulses while busy. Creates "{title} fork" thread. Error "Failed to fork this response." |
| Reply | ghost-muted xs, `reply` 12px, "Reply to this response" (citation feature, defer) |
| Timestamp | `updatedAt`, same format and tooltip as user |

`sent-message` (text an agent sent with `send_to_user`): same as an assistant body, no meta.

### 3.3 Work log line (shared by every work row)

`chat/WorkLog.tsx:39-148`.

| Part | Spec |
|---|---|
| Row | width 100%, radius 6px, padding 2px horizontal, 2px vertical (0 inside an expanded group list) |
| Line | min-height 24px, gap 6px, `text-sm` `leading-relaxed`, not selectable |
| Icon column | 24px wide box, icon wrapper 16px centered, baseline-aligned to the first text line |
| Icon color | `icon-muted` at 70% opacity (light mode: also `brightness(0.6)`); failed: `tool-error-icon/40`; error rows: `destructive`; warnings: `warning` |
| Label | `secondary-label`, single line, truncate (wrap only for errors/approvals). Non-tool-like entries: `foreground/80` |
| Trailing | timestamp (hidden until row hover, then takes space from the label), optional buttons, failure `x` 12px in `tool-error-icon/40` when the icon is an image, chevron |
| Chevron (expandable) | 16px box, `chevron-right` 12px `icon-muted` 70%, rotates 90° when open, 200ms |
| Focus | 2px inset ring `ring/70`, radius 6px |

A single entry (`work` row, `TimelineWorkEntryRow.tsx:290-707`):

- Click or Enter/Space toggles details when there is anything to show.
- Reasoning collapsed: first line of the thought rendered as inline markdown (bold, code, math
  kept; links flattened), capped at 2048 chars.
- Question answer: label is the question text, followed by the answer preview in `foreground`.
- `thread_created`: trailing link-style button "Open chat" in `primary` `text-sm`.
- Subagent notification: trailing "Open subagent".

Expanded details (`WorkLog.tsx:152-192`, `TimelineWorkEntryRow.tsx:661-707`):

| Content | Container |
|---|---|
| Reasoning | indent 28px, max-height 384px, scroll, padding 2×4px, gap 12px, full markdown in `foreground`, follows the bottom while streaming |
| Tool panel | margin-top 2px, margin-bottom 6px, padding-bottom 8px, not indented |
| Plain body (no structured item) | `<pre>` max-height 256px, scroll, mono at code size (13px) `leading-relaxed`, `secondary-label`, wraps. Blocks: "Tool input\n{json}", raw command, detail, changed files, joined by blank lines, skipping duplicates of the label |
| Viewed image | image, max-height 256px (16rem), opens viewer |
| Question answers | indent 28px, margin-top 8px: question in `text-sm` muted, answer indented 12px more, attachments as 80px tall thumbnails |

Structured tool panel (`chat/V2ItemInspector.tsx:245-430`), `text-xs`, vertical gap 8px, mono
blocks at code size with `leading-relaxed`, pre-wrap:

| Item | Panel |
|---|---|
| `command_execution` | the command, syntax-highlighted as shell (`chat/ShellCommandBlock.tsx`) in `foreground/85`; then output (§3.3.1); then "exit {code}" in `destructive` if non-zero |
| `file_change` | relative path mono muted, "+{n}" in `diff-addition`, "-{n}" in `diff-deletion`, outline xs button "Open diff" (opens the diff in the right panel); failed patch text; list "{operation} {old} → {path}" |
| `file_search` | pattern (only if the title did not show it), then a results list |
| `web_search` | queries; results: title link (`foreground`, medium, `external-link` 12px) + snippet muted |
| `dynamic_tool` | "key value" argument lines (key muted), JSON/TS/HTML highlighted, then output |
| `approval_request` | prompt |
| `user_input_request` | each question then "• {option}" lines |
| `todo_list` | "✓ / ◉ / ○ {step}" per step |
| `notification`, `system_notice`, `error` | the text |
| reasoning | italic muted markdown in a box: radius 6px, 1px `border/45`, bg `muted/15`, padding 8px |
| Structured value block | max-height 320px, scroll, `muted-foreground` |

Loading/empty strings: "Loading output…", "No output.", "Couldn't load output: {error}",
"Output is no longer available.", "Earlier output not shown".

#### 3.3.1 Command output

`chat/CommandOutputPanel.tsx:27-104`. Streams while the command runs (server subscription,
only while the row is expanded), then the final output. Max-height 320px, scroll, pinned to
the bottom unless the user scrolls up more than 8px. ANSI colors are parsed into spans
(terminal palette of the current theme). Text `muted-foreground`.

#### 3.3.2 Expanded group list

`chat/TimelineWorkGroupRows.tsx:188-348`. Opening a `work-toggle` inserts a `work` row whose
entries render in their own virtual list:

| Property | Value |
|---|---|
| Max height | min(288px, 50% of window height) + the extra height of any expanded entries inside |
| Estimated item height | 24px |
| Scroll edges | top/bottom fade masks when scrolled away from that edge |
| Follow | stays pinned to the bottom when new entries append and the list was at the bottom |
| State remembered | scroll anchor and which entries are expanded, per thread |
| Region label | "Tool calls" |

### 3.4 `work-toggle` and `work-live`

`TimelineWorkGroupRows.tsx:614-772`.

`work-toggle`: a full-width button line (§3.3) with the summary icon, the summary sentence, and
the hover timestamp. No chevron. Click toggles the group. Failed group (latest tool failed):
accessible label "{label}, tool call failed"; the icon gets the failed tint.

`work-live`: a button that toggles the live group.

- Top line (only if there is a fresh thought and the group is closed): `brain` icon, the
  thought's first sentence (`liveThoughtLine`: a leading bold heading, else text up to the first
  sentence end), up to 4 lines.
- Status line: icon of the current entry + live label (§2.7). Width fits content.
- Active (not failed):
  - `live-tool-shine` on the label: text becomes a gradient of `secondary-label` with a 72px
    `foreground` highlight sweeping left to right, 2.2s, `steps(30)`, infinite.
  - Paused when off screen. Disabled with reduced motion (`index.css:578-608`).

### 3.5 `thinking` and `working`

`TimelineWorkGroupRows.tsx:352-491`.

`thinking`: brain icon + label, with the shimmer overlay (`ActivityShimmerOverlay`,
`TimelineRowParts.tsx:43-60`, `index.css:613-670`). The overlay is a `foreground` copy of the
row under a moving 72px soft mask, 2.2s, `steps(30)`, infinite. Mask alpha along its 72px:
0 at 0, 12% at 10.8px, 55% at 25.2px, 100% at 36px, 55% at 46.8px, 12% at 61.2px, 0 at 72px.
The mask travels from 72px left of the row to 72px past its right edge. With reduced motion the
overlay is hidden.

Label precedence:

1. provider starting label (with the provider icon)
2. "Waiting on {n} subagents"
3. live answer preview (first sentence of the newest assistant text or thought)
4. "Thinking"

No shimmer while an approval is pending.

`working` (the header under your prompt):

| Part | Spec |
|---|---|
| Container | bottom border 1px `border/60`, padding-top 4px, padding-bottom 8px |
| Line | height 24px, padding-x 4px, `text-sm` `leading-relaxed` `muted-foreground` tabular |
| Text | "Working for {t}" where t ticks every second: "{s}s" under a minute, then "1m 5s", "1h 2m 3s". "Working..." if no start time |
| Variants | "Setting up worktree…" (shimmer); compacting: `minimize-2` 12px + "Compacting for {t}" or "Compacting…" (shimmer) |
| Background setup chip | right-aligned ghost-muted micro button: spinner 12px + "Background setup" (opens setup card; worktree spec) |

### 3.6 Turn fold ("Worked for …"), attempt fold, compaction divider

`MessagesTimeline.tsx:1778-1972`.

`turn-fold` (the turn completion divider):

| Part | Spec |
|---|---|
| Container | bottom border 1px `border/60`, padding-top 4px, padding-bottom 8px, padding-end 2px |
| Button | padding-x 4px, radius 6px, `text-sm` `leading-relaxed` `muted-foreground` → `foreground` hover, tabular; label then `chevron-right` 14px (closed) or `chevron-down` 14px (open), gap 4px |
| Timestamp | right-aligned, revealed on hover |
| Lazy history | if hidden work was not loaded, opening shows a 3-line skeleton (12px squares + pill bars 10px tall at 40%/60%/33% width, gap 8px, padding-top 12px) and loads it. Failure appends " · Couldn't load work. Click to retry." |

`attempt-fold`: full-width button, radius 6px, 1px `border/60`, bg `muted/20` (hover `muted/35`),
padding 10×8px, gap 8px: chevron 14px muted, "Superseded attempt" in `text-xs` medium
`foreground/80`, then "Cut off by a steer" in `text-2xs` muted.

`context-compaction`: centered row within the lane, gap 12px, padding-y 4px, `text-xs`
`muted-foreground` (failed: `destructive`):

- Layout: 1px rules on both sides (`border/70`, failed `destructive/30`) around
  `minimize-2` 12px (failed: `circle-alert`) + label.
- Label: "Context compacted {before} → {after} tokens" (arrow is `arrow-right` 12px), or
  "{title} {before} → {after} tokens", else the label.
- Failure: " · {reason}", truncated, 80% opacity.
- Active: shimmer.

### 3.7 Errors in the timeline

Failed provider `error` item (`TimelineWorkEntryRow.tsx:345-428`): work line with
`circle-alert` 16px in `destructive` (usage limit: `warning`), label `text-sm` medium same color,
wraps. "Usage limit reached. Retry after {resetTime}." with "({timezone})". Below, indented
28px: the failure message `text-sm` `foreground/80` pre-wrap, padding-y 4px. Optional outline xs
buttons "Retry" (`rotate-ccw`) for workspace preparation failures. Timestamp always visible.

`system_notice` with level ≠ info and runtime warnings: `circle-alert` in `warning`, label
medium `warning`. Severe failures (non-tool-like failures, provider errors) use `destructive`.

### 3.8 Pending approval row

`chat/TimelinePendingApprovalRow.tsx:16-132`, `chat/ComposerPendingApprovalActions.tsx:27-150`,
`chat/usePendingThreadRequests.ts:354-388`.

Replaces the `approval_request` work entry in place (and approvals with no matching entry
render in the list footer). Also mirrored in the composer.

| Part | Spec |
|---|---|
| Icon | by request kind: command `terminal`, file-change `square-pen`, file-read `eye`, permission `wrench`, mcp-elicitation `hammer`; `icon-muted` |
| Detail | `<code>` block: mono `text-xs` leading 20px `foreground`, pre-wrap, wraps anywhere, max-height 160px scroll, selectable. Falls back to the kind label |
| Below (indent 30px, gap 4px, `text-xs`, padding-bottom 4px) | app name (muted), reason (muted, selectable), then the action line |
| Action line | `shield` 12px + "Waiting for approval" in `warning` · then inline buttons |
| Kind labels | "Command approval", "File change approval", "File read approval", "App permission approval", "App access approval" |
| Detail labels (a11y) | "Command", "File change", "File to read", "Permission request", "App access request" |
| Dead provider | "Provider process is gone. Interrupt or restart the run to respond." in `warning`, no buttons |

Inline buttons (link-style `InlineButton`: medium weight, underline on hover, no padding):

| Button | Tone | Sends |
|---|---|---|
| "Decline" | muted | `decision: "decline"` |
| "Approve" | default (`foreground`) | `decision: "accept"` |
| "More" → menu | muted | "Cancel" → `cancel`; "Always allow this session" → `acceptForSession` |

Providers can supply their own options and labels (shown untranslated, with a
`triangle-alert` 12px and tooltip when an option carries a warning). Everything is disabled
while a response is in flight or the user cannot operate the thread.

Command sent: `thread.respondToApproval { threadId, requestId, decision, feedback? }`. On
failure the thread error banner shows the error or "Failed to submit approval decision.".

Full-access override: an extra muted inline button with the override question, hover popover
explains it (`thread-requests` in client-runtime).

### 3.9 Pending user input

Not a timeline row. The pending question renders in the composer
(`chat/ComposerPendingUserInputPanel.tsx`), which belongs to the composer spec. Once answered,
the request shows in the timeline as a work line labelled with the question text and the answer
(§3.3), expandable to the full Q&A.

### 3.10 Proposed plan card

`chat/ProposedPlanCard.tsx:42-260`, `proposedPlan.ts`, `packages/shared/src/proposedPlanText.ts`.

| Part | Spec |
|---|---|
| Row | padding 4×2px |
| Card | radius 16px, 1px `border/80`, bg `card/70`, padding 16 / 20px |
| Header | space-between, wraps, gap 12px |
| Header left | Badge secondary "Plan" + `<h3>` title `text-sm` medium `foreground` truncate. Title = first markdown heading, else "Proposed plan" |
| Menu button | outline icon-xs (28 / 24px), `ellipsis` 16px, label "Plan actions" |
| Body | margin-top 16px, markdown. The first heading, and a following "Summary" heading, are stripped from the body |
| Collapse | when plan > 900 chars or > 20 lines: show a preview of the first 10 non-empty lines (+ "..."), clipped to 416px, with a 96px gradient at the bottom from `card/95` via `card/80` to transparent |
| Expand button | centered, margin-top 16px, outline sm: "Expand plan" / "Collapse plan" |

Menu items:

| Item | Action |
|---|---|
| "Copy to clipboard" ("Copied!" after) | copy plan + trailing newline. Error toast "Could not copy plan" |
| "Download as markdown" | save as `{slug-of-title}.md` (slug: lowercase, punctuation dropped, non-alnum → "-", default "plan") |
| "Save to workspace" | dialog (below); disabled without write access or workspace |

Save dialog (max-width 576px):

- Title "Save plan to workspace".
- Description "Enter a path relative to `{workspaceRoot}`.".
- Field label "Workspace path", prefilled with the last path or the filename.
- Buttons "Cancel" and "Save" ("Saving..." while busy).
- Toasts:
  - "Enter a workspace path" (empty path)
  - "Plan saved to workspace" with the relative path
  - "Could not save plan"
  - "Workspace path is unavailable" with "This thread does not have a workspace path to save into."

There is no "Implement" button on the card in this fork. Plan follow-up is a composer concern
(the prompt prefix `PLEASE IMPLEMENT THIS PLAN:\n` lives in `proposedPlan.ts:58`).

### 3.11 Event rows and system dividers

`chat/V2LifecycleRow.tsx`, `chat/TimelineSystemDivider.tsx`, `chat/TimelineV2EventRow.tsx`.

System divider:

| Part | Spec |
|---|---|
| Row | gap 8px, padding-y 8px, `text-2xs` `muted-foreground` (danger: `destructive`) |
| Rules | 1px `border/70` flex-1 on both sides |
| Center | pill, padding 8×4px: optional icon 12px + label (medium), then "· {detail}" at 70% opacity (truncate at 320px) |
| With an action | the pill becomes a button: rounded-full, 1px `border/70`, bg `background`, padding 10×4px, hover bg `accent` and `foreground`; tooltip = action label |

| Event | Divider |
|---|---|
| `run_interrupt_result` | `circle-stop` "Run interrupted" · message (wraps) |
| `fork` | `git-fork` "Forked from conversation" (action "Open source conversation") or "Conversation fork" (action "Open fork") |
| `handoff` | `arrow-right-left` "Context handoff" / "Context handoff pending", detail = from model(s) → to model with provider icons; failed = danger |
| New provider session | `eraser` "New session" |
| Model switch | "{old model} {effort} → {new model} {effort}" (only what changed) |
| Parent thread (list header) | `bot` "Subagent of" or `messages-square` "Context from" · "{title}" or "{title} on {environment}"; action opens the parent. Padding-top 4 / 8px |
| `run_interrupt_request` | not rendered |

`thread_created` event: work line with the app logo, "Created thread · {title}" or
"Created thread", trailing "Open chat". As a summary card after a turn's final answer:
radius 8px, 1px `border/60`, padding 12px, 36px icon tile (radius 8px, bg `muted/60`,
`message-square` 16px `secondary-label`), title `text-sm` medium, outline xs "Open chat".

`subagent` event (keep if Tern supports subagents, else skip):

- Layout: a button row, padding 8×6px, radius 6px, gap 10px, hover bg `accent/20`.
- Contents: avatar 24px; title `text-xs` medium; status or progress line `text-2xs` muted
  (failed `destructive`); status; elapsed time; `chevron-right` 14px.
- Statuses: "Queued", "Running", "Waiting", "Idle · resumable", "Completed", "Failed",
  "Stopped".
- Several consecutive subagent events of one provider turn collapse into a group:
  "{count} subagents", up to 3 overlapping avatars, then "+{n}".

`secret_request`, `feedback_request`, `link_request` cards: skip (§7).

### 3.12 Banners above the timeline

`chat/ChatBannerLane.tsx`, `chat/ThreadErrorBanner.tsx:82-148`, `components/ui/alert.tsx`.

Lane: absolute, top 0, spans the chat lane, z-index 30, column, pointer events only on children.

Thread error banner:

| Part | Spec |
|---|---|
| Wrapper | centered, width fits content, max-width min(768px, 100% − 32px), padding-top 12px |
| Alert | radius 12px, 1px border, padding 14×12px, `text-sm`; glass surface (bg `background` at 80% with 12px blur, 16px in dark, tinted 4% with the variant color) |
| Variant error | border `error/32`, text `error-foreground`, description `error-foreground/80`, icon `error` |
| Variant warning (usage limit) | border `warning/32`, text `warning-foreground`, icon `warning` |
| Row | icon `circle-alert` 16px aligned to first line, gap 8px, text flex-1, actions at the end |
| Text | if the error is JSON `{ error: { message } }`, show that message collapsed and the full text on expand (`ExpandableText`) |
| Dismiss | ghost icon-xs `x`, label "Dismiss error". Dismissal is remembered for the session per (thread, error) |

The ChatGPT usage-limit variants ("ChatGPT usage limit reached", "Copy request for admin")
are provider-specific; Tern keeps the generic warning variant. The provider status banner in the
same lane belongs to the providers spec. Branch-mismatch, stop/force-stop and goal banners
(`ThreadActivityBanners.tsx`) render in the composer banner stack, composer spec.

### 3.13 Misc rows

- "Load earlier turns" (`MessagesTimeline.tsx:1569-1600`): full-width button, gap 12px,
  padding-y 6px, `text-xs` `muted-foreground` (hover `foreground`), rules `border/70` on both
  sides; "Loading earlier turns…" while loading; an error line under it, centered `text-xs`
  muted.
- `viewed-images`: wrap row, gap 8px, padding-x 2px, each image max-height 160px, opens the
  viewer as a gallery.
- `worktree-setup`: setup card, worktree spec.

### 3.14 Diff stat chips

`chat/DiffStatLabel.tsx`. In this fork the timeline does not render per-turn diff chips; turn
diff summaries only decide which user messages get "Edit from here". The chip is used by the
diff panel. Spec for reuse: two mono spans "+{n}" in `diff-addition-foreground` and "-{n}" in
`diff-deletion-foreground`. Layout "aligned" is a 2-column grid of 4ch each, gap 8px,
right-aligned, tabular; "inline" is gap 4px. Counts compact: 999, "1.2k", "12k", "1.5m", "2b".
Accessible label "{additions} additions, {deletions} deletions".

---

## 4. Markdown

`components/ChatMarkdown.tsx:1140-1900, 2074-2137`, `index.css:1555-1585, 1815-2365`,
`chat/MarkdownCodeBlock.tsx`, `chat/MarkdownTable.tsx`, `chat/MarkdownFileLink.tsx`,
`markdown-links.ts`, `packages/shared/src/markdownLinks.ts`, `packages/shared/src/fileLinks.ts`.

GFM: tables, task lists, strikethrough, autolinks, footnotes; GitHub alerts; math (KaTeX,
defer); raw HTML only in assistant text.

### 4.1 Root

| Property | Value |
|---|---|
| Font | interface sans, `text-sm` 14px, `leading-relaxed` (22.75px) |
| Color | `foreground` at 90% (+ contrast boost setting / 10) |
| Wrapping | `overflow-wrap: anywhere`, `word-break: break-word` |
| First child | margin-top 0; last child margin-bottom 0 |

### 4.2 Blocks

| Element | Spec |
|---|---|
| p, ul, ol, blockquote, pre, table container | margin 10.4px 0 (0.65rem) |
| h1 / h2 / h3 / h4–h6 | 20 / 18 / 16 / 14px; weight 600; line-height 1.3; margin 20px top, 8px bottom; color `contrast-foreground`; h6 `contrast-muted-foreground` |
| ul | padding-start 20px; disc → circle → square by depth |
| ol | padding-start max(20px, width of "0. " in the sans font), widened for multi-digit numbers; decimal → lower-alpha → lower-roman by depth; tabular markers |
| li + li | margin-top 4px |
| Task item | no marker; checkbox pulled into the gutter, margin-end 0.35em, vertically middle |
| blockquote | inline-start border 2px `currentColor` 35%, padding-start 12.8px, color inherit |
| GitHub alert (`> [!NOTE]` etc.) | margin-y 4px, inline-start border 2px, padding-start 12px; title line: icon 14px + label, medium, gap 6px |
| hr | default; dark mode border `contrast-muted-foreground` |
| strong | 700 |
| Footnotes | margin-top 20px, top border 1px `contrast-border`, padding-top 12px, `contrast-muted-foreground`, 12px; refs are 11px bold pills min-width 16px, radius 4px |

GitHub alert kinds:

| Kind | Label | Icon | Border | Title color |
|---|---|---|---|---|
| note | "Note" | `info` | blue-500/70 | blue-600 (dark blue-400) |
| tip | "Tip" | `lightbulb` | emerald-500/70 | emerald-600 (dark emerald-400) |
| important | "Important" | `message-square-warning` | purple-500/70 | purple-600 (dark purple-400) |
| warning | "Warning" | `triangle-alert` | amber-500/70 | amber-600 (dark amber-500) |
| caution | "Caution" | `octagon-alert` | red-500/70 | red-600 (dark red-400) |
| insight | "Insight" | `star` | `info` | `info` |

### 4.3 Inline

| Element | Spec |
|---|---|
| Inline code | 0.85em, padding 0.1em 0.35em, radius 4px, 1px transparent border, bg `contrast-foreground` 10%, text `contrast-foreground`; clones decoration across line breaks |
| Link | `info-foreground`, no underline; hover/focus: dotted underline (1.5px dots every 4px along the baseline) |
| External link | opens in the system browser or the integrated browser per setting; right-click menu (`chat/externalLinkContextMenu.ts`) |
| Image | inline-block, max 480×480px (`min(100%, 30rem)`), radius 8px, 1px `border/40`, contain; click opens the viewer; workspace paths load through the server |

### 4.4 Code blocks

`MarkdownCodeBlock.tsx:43-280`.

| Part | Spec |
|---|---|
| Frame | margin-y 10.4px, radius 8px, 1px `border/70`, bg `secondary`; dark: border transparent, bg `input/32`; themed palettes use `code-background` / `code-foreground`; `leading-snug` (1.375); always LTR |
| Header | padding 6px top, 6px right, 0 bottom, 12px left; not selectable; color `contrast-foreground` 72% |
| Header title | mono `text-2xs` (11px), gap 6px |
| Title, fence has a file name (```ts title="a.ts"```) | file icon 14px + file name |
| Title, language has a specific file icon | file icon 14px, tooltip = language, label "Language: {language}" |
| Title, otherwise | language id as text |
| Toolbar | gap 2px, buttons icon-xs (28 / 24px), icons 12px: wrap, run, copy |
| Wrap toggle | `wrap-text`; variant secondary when on, ghost-muted when off. Labels "Wrap lines" / "Disable line wrap". Default from setting `wordWrap` |
| Run | `play`, "Run in terminal". Only for sh/bash/zsh/fish/shell/powershell/pwsh fences that are closed, not streaming, non-empty, no trailing backslash, no control/format chars. Sends the trimmed command to the project terminal |
| Copy | `copy` → `check` for 1.2s, "Copy code" / "Copied". Copies without the trailing newline |
| pre | padding 12.8px 14.4px; mono at `--font-size-code` (13px); horizontal scroll with thin scrollbar unless wrapped; wrapped: pre-wrap, break anywhere |
| Highlight | Shiki. Dark theme `pierre-dark`; light theme a darkened `pierre-light` (`lib/readableSyntaxTheme.ts`). Background forced transparent. Plain-text fences (text, txt, plain, plaintext) use the inherited color |
| Streaming | highlighted incrementally line by line; invisible plain fallback until the grammar loads |

### 4.5 Tables

`MarkdownTable.tsx:23-80`, `index.css:2300-2365`.

| Part | Spec |
|---|---|
| Container | horizontal scroll area with edge fades and an always-visible horizontal scrollbar (thumb `muted-foreground` 60%, hover 80%) |
| Table | width 100%, collapsed borders, 14px, words wrap only at word boundaries |
| Cells | padding 7.2px 12px; first cell no start padding, last cell no end padding; top-aligned; start-aligned; max-width 384px |
| Header cells | weight 600, padding-y 8.8px, bottom border 1px `contrast-border` 60% |
| Body cells | bottom border 1px `contrast-border` 60% |
| Copy | under the table, right-aligned, margin-top 2px: ghost-muted icon-xs `copy` button, "Copy table" / "Copied", opens menu "Copy as Markdown", "Copy as CSV", "Copy as HTML" (HTML also puts `text/html` on the clipboard) |

### 4.6 File links

How text becomes a clickable file chip (`markdown-links.ts:65-150`,
`packages/shared/src/markdownLinks.ts:202-600`):

1. **Markdown links** `[x](path)`. The destination is a file link when it parses as a path:
   - `file:` URL, absolute POSIX/Windows path, or `~/`, `./`, `../` prefix
   - a relative path whose segments look like file names
   - a bare file name with an extension
   - an optional position suffix `:line`, `:line:col` or `:line-endLine`

   It is not a file link when it has a URL scheme, starts with `#` or `//`, or is a network path
   outside the workspace. Relative paths resolve against the thread's working directory.
2. **Inline code** `` `src/app.ts:12` ``. Becomes a chip when the trimmed text, all of:
   - has no backtick and no whitespace (except an absolute path whose words are plain names)
   - contains a path separator or a position suffix
   - has no `...`/`…` segment
   - has an explicit path shape, or a final segment with a file extension, or a known
     extensionless file name (Makefile, Dockerfile and similar)

   It also must not look like a hostname or a version number ("1.2.3"). Otherwise it stays
   inline code.

Chip (`MarkdownFileLink.tsx:330-410`, `components/ContextChip.tsx`):

| Part | Spec |
|---|---|
| Box | inline-block, nowrap, ellipsis, medium weight, radius 4px, color `--context-chip-mention` (document types use their own: pdf, word, spreadsheet, slides, epub tokens) |
| Icon | file-type icon 1em, margin-end 0.2em, baseline −0.125em |
| Label | `{basename}` plus " · {parent suffix}" when two links share a basename, plus " · L{line}", " · L{line}–{end}" or " · L{line}:C{col}"; NFC-normalized; underline on hover |
| Tooltip | full path (code-style tooltip) |

Activation:

| Input | Action |
|---|---|
| Click | open in the right panel's file preview (workspace files), or the integrated browser when that is the only option |
| Cmd/Ctrl+click | open in the preferred editor at the line |
| Right-click, or Shift+F10 / context-menu key | native menu |

Native menu items, in order, when available:

1. "Preview media"
2. "Open in {editor}"
3. "Open in integrated browser"
4. "Reveal in {file manager}"
5. "Copy relative path"
6. "Copy full path"

Errors: "Unable to open file", "Unable to open file in browser", "Unable to reveal file",
"Failed to copy relative path" / "Failed to copy full path". A chip with no primary action is a
button that opens the menu, label "File options for {label}".

Chips can be dragged out as native files.

---

## 5. Scroll behavior

`chat/useTimelineFollow.ts`, `chat/timelineViewport.ts`, `chat/timelineScrollAnchoring.ts`,
`chat/useTimelinePositionRestore.ts`, `MessagesTimeline.tsx:1496-1562`.

### 5.1 Follow modes

| Mode | Meaning |
|---|---|
| following-end | the view stays pinned to the end as content grows |
| anchoring-new-turn | a just-sent message is held near the top, the response grows below it |
| free-scrolling | the user scrolled away; nothing moves the view |

Rules:

- "At end" means `contentHeight − scrollTop − viewportHeight ≤ 40px`
  (`MessagesTimeline.logic.ts:359-371`). The list's own isAtEnd is only a fallback.
- While following, content growth keeps the view at the end:
  - new data, row resizes and layout changes all count; footer growth does not
  - smooth scroll while working, instant otherwise and with reduced motion
- Any user scroll (wheel, drag on the scrollbar within 16px of the edge, keys, touch) that moves
  away from the end releases follow. Downward input inside the end band does not release,
  because streaming growth makes "at end" flicker false for a frame
  (`timelineViewport.ts:216-243`).
- Scrolling back to the end re-arms follow.
- Sending a message while following: when the new run starts, its user message is anchored
  24px below the top of the viewport (`CHAT_TIMELINE_ANCHOR_OFFSET`) with reserved empty space
  below so the anchor can sit there. The space is released as the response fills it, then
  normal following resumes.
- PageUp/PageDown typed in the composer scroll the timeline (`pageScrollController.ts`).
- Footer growth (a queued turn, a new status line) scrolls the list only if it was following.

### 5.2 "Scroll to end" pill

`chat/TimelineOverlays.tsx:27-56`, `ChatView.tsx:3342-3352, 3760-3766`.

| Property | Value |
|---|---|
| Shown | not following and find is closed. Showing is debounced 150ms (avoids flashes during thread switches); hiding is immediate |
| Position | absolute, centered across the chat lane, bottom = composer clearance + 4px, wrapper padding-y 6px, z 30 |
| Button | glass variant xs: height 28 / 24px, rounded-full, 1px `border/60` (hover `border`), bg `background` 80% with 12px blur (16px dark), small shadow, `text-sm` / `text-xs`, padding-x 7px, gap 6px |
| Content | `chevron-down` 14px + "Scroll to end", then muted " · {summary}" when present |
| Summary | parts joined " · ": the child/subagent waiting label, "{count} queued" |
| Click | re-arm follow, drop any anchor space, scroll to end (animated unless reduced motion), then correct against measured heights for up to 60 frames. Pointer-down does not steal focus from the composer |

### 5.3 Position restore per thread

`timelineScrollAnchoring.ts:115-151`. On leaving a thread the app remembers, in memory (LRU
of 100 threads, not persisted):

- the first visible row id, the offset within that row, and the raw scroll offset
- `atEnd` (true if following)
- disclosures: open turn folds, open work groups, open attempt folds, and the per-group scroll
  anchors and expanded entries

On return:

- `atEnd` true → open at the end.
- Otherwise scroll to the saved row + offset, correcting until the row holds still for two
  frames or 60 frames pass.
- Any user navigation cancels the restore.
- While switching threads, the previous thread's last ready rows are shown until the new
  thread's rows load (`ThreadTimeline.tsx:44-56`).

---

## 6. Find bar and minimap

### 6.1 Find in thread (Mod+F): core, simplify

`chat/ThreadFindBar.tsx:35-156`, `chat/useThreadFind.ts`, `chat/threadFindHighlights.ts`,
`index.css:434-450`, keybinding `packages/shared/src/keybindings.ts:119`
(`mod+f`, when not in terminal or preview).

Only enabled for server threads whose server advertises `threadFind`.

Layout:

| Property | Value |
|---|---|
| Position | absolute, top 12px, right 12px of the canvas, width min(280px, 100% − 24px), z 40 |
| Open animation | fades in over 150ms |
| Shape | popover-style input group, size lg |
| Input | type search, placeholder "Find in thread", max 200 chars, no spellcheck |
| Count | `text-xs` tabular, min-width 40px, `muted-foreground`; `destructive` when there are no results |
| Buttons | ghost icon-xs `chevron-up` "Previous match", `chevron-down` "Next match", `x` "Close find"; a ghost xs "Retry" on error |

Count text:

| State | Text |
|---|---|
| Results | "{n}/{total}" |
| No results | "0/0" |
| Counting | "1/…" |
| Searching | "Searching…" |
| Error | "Search failed" |

Keys: Enter = next, Shift+Enter = previous, Escape = close (focus returns to the composer).
Mod+F again refocuses and selects the query. Prev/next are disabled with 0 matches or while
searching.

Search runs on the server (`orchestration.threadFind`). Rows marked `data-thread-find-text`
are searched; `data-thread-find-ignore` parts (code block headers, chips) are not.

Highlighting:

| Highlight | Light | Dark |
|---|---|---|
| All matches, text | oklch(0.216 0.028 77.4) | oklch(0.970 0 0) |
| All matches, background | oklch(0.915 0.11 85) | oklch(0.55 0.1 85) |
| Active match, background | oklch(0.774 0.152 74.3) | same |

Themes can override all three. Folded content (long user messages, collapsed plans, folded
agent messages) reveals itself when the active match is inside. While find is open, follow mode
and anchoring are suspended and the "Scroll to end" pill is hidden.

Verdict: **core**. Users expect Ctrl+F in a long thread. For Tern, do it client-side over row
text, keep the bar, counts, keys and highlight colors, and drop the server RPC and
progressive counting.

### 6.2 Timeline minimap: slop, defer

`chat/TimelineMinimap.tsx:33-313`, `chat/timelineMinimapItems.ts`,
`MessagesTimeline.logic.ts:315-505`.

A rail on the left edge with one 24×2px rounded strip per user message.

| Property | Value |
|---|---|
| Rail | 72px wide; extends up behind the header; z 40 |
| Visibility | always visible when the side gutter is ≥ 48px; otherwise hidden and fades in on hover (mouse only) |
| Rail height | min((n − 1) × 8px, max(64px, 100% − 236px)), vertically centered |
| Strips | `muted-foreground/25`, hovered `muted-foreground/60`; in-view turns get a `foreground/55` overlay |
| Hover fisheye | widths 100%, 66.7%, 41.7%, 33.3% by distance from the hovered strip |
| Hit strip | 32px from the left, up to 40px wide |
| Hover card | 320px glass card, radius 12px, padding 12px: the user message (`text-sm` medium, one line), then the turn's final answer clamped to 3 lines muted |
| Navigation | prev/next turn buttons ("Previous turn", "Next turn"); click a strip to jump with a 24px offset; keyboard ↑/↓/Home/End/Enter. Label "Jump to message: {text}" ("User message" fallback) |
| Minimum | 2 user messages |

Verdict: **slop**. Decorative, mouse-only, hidden on most widths, and it duplicates scrolling.
Skip for v1.

---

## 7. Out of scope (skip on purpose)

| Item | Where | Why |
|---|---|---|
| HTML render frames (`html-render` rows, HTML preview lane, `--html-preview-*`) | `HtmlRenderFrame.tsx`, `ChatCanvas.tsx` | dropped |
| MCP apps (`mcp-app` rows, MCP sign-in action in notices) | `McpAppFrame.tsx`, `McpSignInAction.tsx` | dropped |
| Mermaid diagrams | `MarkdownMermaidCodeBlock.tsx`, `MermaidDiagram.tsx` | dropped; render as a plain code block with language "mermaid" |
| Guest sharing (guest bubble style, "{name} (guest)", share indicator) | `guestMessageStyle.tsx`, `GuestShareControls.tsx` | dropped |
| T3 Connect / linked environments ("From {origin}" badge, "on {environment}", remote subagents) | `ChatHeader.tsx:399-413`, `V2LifecycleRow.tsx` | dropped |
| Cloud: "Copy prompt link", Clerk | `TimelineUserMessageRow.tsx:885-897` | dropped |
| Voice and audio attachments | `InlineAudioPreview`, speech | dropped |
| Mobile: coarse-pointer variants, safe-area insets, draft hero mobile transitions, 44px touch targets | throughout | dropped |
| Telemetry, screen-reader activity announcer | `ThreadActivityAnnouncer.tsx` | dropped (keep basic a11y labels if cheap) |
| Citations: "Reply to this response", selection toolbar, citation chips | `AssistantSelectionToolbar.tsx`, `AssistantCitation*.tsx` | defer |
| Timeline minimap | §6.2 | slop |
| Secret, feedback and link request cards | `SecretRequestCard.tsx`, `FeedbackRequestCard.tsx`, `LinkRequestCard.tsx` | defer (cloud/integration flows) |
| T3 MCP tool sentences and icons | `t3ToolSentences.ts`, `toolPresentation.ts` | no T3 MCP server in Tern |
| Artifact template cards, "★ Insight" blocks | `ChatMarkdown.tsx:297-345` | defer |
| Snap-shot image attachments | `SnapShotAttachmentDetails.tsx` | defer |
| Math (KaTeX) | `ChatMarkdown` math extension | defer; show source |
| ChatGPT-specific usage limit banner content | `ThreadErrorBanner.tsx:100-135` | provider-specific; generic warning only |
| Provider/remote favicons as tool icons (`website`, `themed-logo`, `native-app` tool icons) | `TimelineToolActivityIcon.tsx:79-230` | defer; always use the fallback Lucide icon |
| Dragging file chips out as native files | `useNativeFileDrag.ts` | defer |

---

## Open questions

1. Header controls. The brief lists open-in-editor, git actions and a diff toggle in the header,
   but this fork moved them into the thread details panel (§1.4). Should Tern match the fork, or
   upstream T3 Code's older header?
2. Thread find runs on the server here. Does Tern's backend have a search RPC, or is
   client-side search over loaded rows enough (§6.1)?
3. Subagents (`subagent` events, groups, "Waiting on {n} subagents", the "Subagent of" divider):
   does Tern's backend emit them, or should all of it be cut?
4. Syntax highlighting. Shiki with `pierre-dark` and a darkened `pierre-light` has no direct
   GPUI equivalent. Pick tree-sitter plus a palette ported from those two themes, or reuse Zed's
   themes?
5. File-type icons (Pierre icon set) appear in code block headers, file chips and attachments.
   Which icon set does Tern ship? This likely belongs to the theme/tokens spec.
6. Turn folds hide all work of finished turns by default ("Worked for 1m 3s"). That is a big
   behavioral choice, and it differs from upstream T3 Code, which shows work logs inline. Confirm
   Tern follows the fork.
7. The worktree setup row and the background setup chip need a worktree spec. Is one planned?
