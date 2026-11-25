# 06 Settings

Source of truth: `forks/t3code/apps/web/src/` (paths below are relative to it unless they start
with `packages/` or `apps/`). Px assume the desktop window is at or above Tailwind `sm` (640px),
so `sm:` values win; `max-md:` (below 768px) rules are noted where they change layout.
1 Tailwind unit = 4px, `text-xs` 12px/16px, `text-2xs` 11px, `text-sm` 14px/20px, `text-base`
16px/24px. Colors are token names from spec/01.

Strings are English only (i18n dropped). Every user-visible string below is verbatim; keep
typographic quotes and `…` as written.

Page index (nav order): General 2, Appearance 9, Chat 3, Diffs & pull requests 4,
Keybindings 11, Providers 10, Threads 5, Projects 12, Source control 14, Worktrees 13,
Maintenance 6, About 7 (+ Open source licenses 7.1), Archive 8. Dropped pages 15, persistence 16,
open questions 17.

---

## 1. Shell

### 1.1 Modal (`components/AppRouteModal.tsx:110-150`, `routes/_chat.settings.tsx:143-176`)

Settings is a route-level modal over the chat. `/settings` redirects to `/settings/general`.

| Part | Spec |
|---|---|
| Backdrop | full window, `dialog-backdrop` (spec/01), fade 200ms. Clicking the backdrop closes (press must start and end on it) |
| Viewport padding | 24px all sides at >= 640px; top is `max(24px, native titlebar height)`; 40px at >= 1024px. Below 640px the modal fills the window |
| Surface | `bg-background`, `text-foreground`, 1px `border`, radius 16px, `shadow-2xl` at 40%. Max 1280 x 960px, otherwise fills the padded viewport |
| Open/close anim | opacity 0 -> 1 and scale 0.985 -> 1, 150ms ease-out (reverse on close) |
| Close | Esc (ignore key-repeat), backdrop click, or the X button. Blur any focused text field first so commit-on-blur inputs save |
| Layout | row: nav (left) + content column (flex 1) |

### 1.2 Content header (`routes/_chat.settings.tsx:155-167`)

Height 48px, `border-b`, padding-left 20px, padding-right 8px, gap 12px, items centered.

- Left: breadcrumb (`SettingsBreadcrumb.tsx`, `WorkspaceBreadcrumb.tsx:11-73`): `Settings` `/` `<Page title>`.
  14px, `font-medium`, gap 12px. `Settings` is `text-muted-foreground`; separator `/` is
  `text-icon-muted`; current page `text-foreground`, truncates. Detail pages map to their own label:
  `/settings/open-source-licenses` -> `Open source licenses`.
- Right (gap 4px): on General only, **Restore device defaults** button (ghost, size xs = 24px high,
  `RotateCcw` icon 14px, label hidden below 640px; disabled when nothing differs from defaults).
  Then the close button: ghost `icon-sm` (28px square), `X` icon, tooltip `Close (Esc)` below.

Restore device defaults (`components/settings/SettingsPanels.tsx:480-795`): builds a list of
changed setting labels, then a destructive confirm:
`Restore default settings?` + newline + `This will reset: {comma-joined labels}.`
On confirm it resets theme to `system`, follow-system on, clears theme mix, then writes defaults
for every client/appearance/chat/diff/sidebar/confirmation/typography key plus these server keys:
`responseStreamingMode, enableProviderUpdateChecks, continueThreadsAfterServerUpdate,
backgroundActivity, keepAwakeWhileAgentsWork, backgroundActivityProfile,
automaticGitFetchInterval, providerHealthRefreshInterval, defaultThreadEnvMode,
newWorktreesStartFromOrigin, addProjectBaseDirectory, ghqCloneLayout, textGenerationModelSelection,
threadTitleInstructions, sidebarAutoSettle*, autoResumeLimitedThreads, snoozeLimitedThreads,
snoozePresets`. Theme failure toast: `Couldn’t restore theme settings` / `Try again.`
The content area is remounted after a restore (key bump) so every draft resets.

### 1.3 Left nav (`components/settings/SettingsSidebarNav.tsx:261-446`, `settingsSearch.ts:99-191`)

`nav`: width 240px (56px icon rail below 768px), `border-e`, `bg-sidebar`,
`text-sidebar-foreground`. Inner group padding 8px (`--sidebar-content-inset`). Scrolls with
hidden scrollbars and a 12px top/bottom fade.

Search box (top): 32px high, radius 6px, padding 6px 8px, gap 8px, 14px `font-medium`
`text-sidebar-muted-foreground`; hover `bg-sidebar-row-hover`. `Search` icon 16px at 80% opacity,
input placeholder `Search` (`text-sidebar-muted-foreground`). Right side shows a `Kbd` `/` hint,
or while there is a query a ghost-muted `icon-micro` (20px) clear button with a 12px `X`.
`/` (no modifiers, not while typing in a field) and `Cmd/Ctrl+F` focus and select it.

Groups: column, gap 16px between groups, gap 4px between heading and items, items gap 4px.
Group heading: padding-x 8px, 12px `font-medium`, `text-sidebar-muted-foreground` at 80%.
"Preferences" and "More" headings are hidden (space only).

Nav item (`components/ui/sidebar.tsx:879-901`): 32px high, radius 8px (`--control-radius`),
padding-x 10px, gap 8px, 14px `font-medium` `text-sidebar-muted-foreground` at 80%, icon 16px in
`--sidebar-icon-color`. Hover: `bg-sidebar-row-hover`, text + icon `text-sidebar-foreground`.
Pressed: `bg-sidebar-row-active`. Active: `bg-sidebar-row-selected`, `text-sidebar-foreground`,
icon `text-sidebar-foreground`. Click navigates with replace (no history entry). Arrow keys move
between items (`settingsSectionNavigation.ts`).

Tern nav (T3 order with dropped entries removed; lucide icon names):

| Group heading | Label | Path | Icon |
|---|---|---|---|
| (hidden) Preferences | General | /settings/general | Settings2 |
| | Appearance | /settings/appearance | Palette |
| | Chat | /settings/chat | MessageSquareText |
| | Diffs & pull requests | /settings/diffs | FileDiff |
| | Keybindings | /settings/keybindings | Keyboard |
| Agents | Providers | /settings/providers | Bot |
| | Threads | /settings/threads | MessagesSquare |
| Code | Projects | /settings/projects | PanelsTopLeft |
| | Source control & issues | /settings/source-control | GitBranch |
| | Worktrees | /settings/worktrees | FolderTree |
| (hidden) More | Maintenance | /settings/maintenance | Wrench |
| | About | /settings/about | Info |
| | Archive | /settings/archived | Archive |

Dropped from the nav: Voice, SnapShots (Preferences); Browser, Devices, Scheduled tasks (Agents);
the whole Connections group (Environments, Sharing & access, Secrets); Labs (More). With no
Connections group, Agents and Code stay as T3 renders them. Consider renaming "Source control &
issues" to "Source control" since issue trackers are dropped (open question).

Detail pages highlight their parent: `/settings/open-source-licenses` -> About.

Search results (replace the groups while a query is non-empty): `size sm` menu rows
stretched to min-height 40px, items top-aligned, each with the page icon, the setting title (14px
`font-medium` `text-sidebar-foreground`, truncate) over the page label (11px
`text-sidebar-muted-foreground` at 75%). Up/Down wraps, Enter opens, mouse hover moves the
highlight, Esc clears. Empty: `No settings found` (12px, centered, padding 24px 8px). Opening a
result navigates to the page with `#<settingId>`; the target scrolls to center and pulses an
inset 2px `primary` at 70% ring, 650ms ease-in-out, twice, radius 12px
(`index.css:950-964`). Folds (`Advanced`, folded sections) containing the target open first.
The search index is `SETTINGS_SEARCH_ITEMS` in `settingsSearch.ts:200-1560`; titles used below
come from it. Searchable keybinding rows use id `keybinding-<command>`.

Footer: T3 Connect sign-in/avatar. Dropped.

### 1.4 Page container (`components/settings/settingsLayout.tsx:625-669`, `components/WorkspacePageContainer.tsx`)

- Scroller: flex 1, vertical scroll, stable gutter both sides, top fade under the header.
- Column: centered, max-width 896px (`readable`; `wide` 1024, `expanded` 1152), padding
  24px 24px 48px (20px sides below 640px), vertical gap 32px between sections.
- First child is the scope sentence (1.7), then sections.

### 1.5 Section and group (`settingsLayout.tsx:213-265`, `SettingsGroup.tsx`)

`SettingsSection(title, icon?, titleAction?, headerAction?, variant = grouped | plain)`:
- Gap 10px between header and card.
- Header row: min-height 28px, padding-x 16px (12px below 640px), space-between, wraps with
  gap 16px x 4px. Title `h2`: 16px `font-semibold` `text-foreground`, optional 16px icon gap 8px.
  `headerAction` sits right, min 28x28.
- Card (`grouped`): radius 12px, 1px `border` at 60%, `bg-card` at 40%, `shadow-xs` at 5%.
  Children are separated by a 1px top border `border` at 50%; rows inside lose their own radius.
- `plain`: no card, children stacked with 4px gap.

### 1.6 Row (`settingsLayout.tsx:313-583`)

`SettingsRow(title, description?, status?, resetAction?, control?, children?)`:

- Padding 12px 16px (12px sides below 640px). With `children`: padding-top 12px, bottom 4px.
- Container query: when the row is at least 512px wide, it is a 2-column grid
  `[1fr | auto]`, items centered, column gap 32px. Narrower: column, gap 12px, control full width.
- Left column (gap 4px):
  - Title line: min-height 20px, gap 6px: `h3` 14px `font-medium` `text-foreground`, then a 20x20
    inheritance marker slot (project scope only, see 1.7), then a 20x20 reset slot.
  - Description: 12px, line-height 1.5, `text-muted-foreground`, max-width 672px, `text-pretty`.
  - Status (optional): padding-top 2px, 12px `text-muted-foreground`.
- Right column: flex, gap 8px, justify end. Select triggers get min-width 160px here.
- `children` render below the grid, full width (lists, textareas).
- Disabled row (`aria-disabled`): 64% opacity, all text muted.
- Inert control (no write permission / locked): control at 50% opacity, not interactive, wrapped
  in a tooltip. In Tern only these two messages can apply: `Saved on this device. Select All projects to
  change it.` (device-only setting viewed in a project scope) and
  `Environment-wide setting. Select an environment to change it.` (replace with "Select All
  projects" wording, see open questions).

Reset button (`settingsLayout.tsx:585-623`): ghost-muted `icon-micro` 20x20, radius 4px,
`Undo2` icon 12px, tooltip `Reset to default` (project override: `Reset to inherited value`),
aria `Reset {title} to default`. Shown only while the value differs from its default. Clicking
keeps focus on the row.

Advanced fold (`AdvancedSettings.tsx`): last item inside a card. Trigger min-height 40px, padding
x 16px, gap 6px, 12px `font-medium` `text-muted-foreground` (hover `text-foreground`),
`ChevronRight` 14px rotating 90deg in 150ms, label `Advanced`. Bottom corners rounded 12px when
closed. Panel: rows separated by `border` 50%, starts with a top border. Closed by default.

Folded section (`FoldedSettingsSection.tsx`): a card whose header is a trigger (min-height 44px,
padding-y 8px, `ChevronRight` 16px `text-muted-foreground`, title 14px `font-medium`, summary 12px
muted truncate) with an optional right control; body rows separated by `border` 50%.

Policy tooltip: ghost-muted `icon-micro` with `Info` 14px, 200ms delay, popup on top.

### 1.7 Scope sentence (`SettingsScopeSentence.tsx:51-141`, `settingsScope.ts`)

Pages that hold project-overridable settings start with a 14px `text-muted-foreground` line,
padding-x 16px, wrapping with gap 6px x 4px:

`Applying settings for [All projects v] across [All environments v]`

Each `[...]` is an outline `sm` button (28px high, max-width 288px) with label, and a 14px
`ChevronDown` at 70% opacity; it opens a menu (a searchable combobox, 320px wide, when there are
more than 6 options). Project menu: `All projects` then each project (`displayName`, secondary
line is the path; archived ones say `Archived · <path>`). Search placeholder `Search projects`,
empty `No matches found.`

Tern has one environment, so render only `Applying settings for [project v]` (drop the
environment and checkout menus). Not shown on: General, Appearance, Chat, Diffs, About (device-only
pages, `SETTINGS_DEVICE_ONLY_PATHS`).

When a project is selected, rows for project-scoped keys show an inheritance marker
(`SettingInheritance.tsx`) with these summaries: `Overridden for this project`,
`Inherited from the repository's t3.json`, `Inherited from environment`, `Set on the environment`,
`Built-in default`. The marker is hidden until hover/focus when the value is a default or set
right here. Project overrides live in `projectSettingsOverrides[projectId]` (section 16).

### 1.8 Control sizes used on these pages (`components/ui/*`)

| Control | Size |
|---|---|
| Switch (default) | track 30 x 18px, padding 2px, thumb 14px circle; on `bg-primary`, off `bg-input`; thumb `bg-background` (dark + off: `bg-muted-foreground`); 200ms |
| Select trigger `sm` | min-height 28px, padding-x 9px, gap 6px, radius 8px, outline style (`border-input`, `bg-popover`, dark `bg-input` 32%), 14px |
| Select popup items | min-height 28px, padding 4px 8px, radius 4px, 14px; highlighted `bg-accent`; selected `bg-foreground` 8%. Popups align to the trigger's end, not item-aligned |
| Input `sm` | inner 26px + 1px border = 28px, padding-x 9px, radius 8px |
| Button `sm` | 28px high, padding-x 9px, gap 6px, 14px `font-medium`, 16px icons |
| Button `xs` | 24px high, padding-x 7px, 12px, 14px icons |
| Button `compact` | 28px, padding-x 7px, radius 6px, 12px, 14px icons |
| Button `icon-sm` / `icon-micro` | 28px / 20px square (micro radius 4px, 12px icon) |
| Variants | `outline` (border-input, bg-popover, hover `bg-accent` 50%), `ghost`, `ghost-muted` (muted text, hover `bg-accent` + foreground), `destructive-outline` (destructive text, hover destructive 4% bg) |

Most selects on these pages are 160px wide (`sm:w-40`); others noted per row.

---

## 2. General (`SettingsPanels.tsx:846-1231`)

Device-only page (no scope sentence). All keys here are client settings (section 16).

| Section | Title | Description | Control | Options (label = stored) | Default | Affects |
|---|---|---|---|---|---|---|
| Notifications | Thread notifications | `System alerts when a thread finishes, fails, or needs input or approval. Applies to this device while T3 Code is open.` | Select 224px | `Off`=off, `Notifications only`=notifications, `Sound only`=sound, `Notifications with sound`=notifications-and-sound | off | OS notification and/or sound on thread complete/fail/needs input. `notificationMode` |
| | In-app notifications | `Show a toast when another thread finishes, fails, or needs input or approval while this app has focus.` | Switch | | false | toast for background threads. `inAppNotificationsEnabled` |
| Sidebar | Project grouping | `Combine matching repositories across environments.` | Switch | on = last enabled mode (`repository` or `repository_path`), off = `separate` | repository (on) | sidebar grouping. `sidebarProjectGroupingMode`. Tern has one environment; keep for parity, it still merges same-repo checkouts |
| | Project order | `Order of projects in the sidebar project picker and command palette.` | Select 176px | `Last user message`=updated_at, `Created at`=created_at, `Manual`=manual | updated_at | `sidebarProjectSortOrder` |
| | Working section | `Fold working and monitoring threads into a Working section. They return to the top of the inbox when they need you. The boring sidebar in Labs has no Working section.` | Switch | | true | sidebar Working shelf. `sidebarWorkingShelfEnabled`. Drop the last sentence (Labs dropped) |
| Confirmations | Unpin confirmation | `Ask before unpinning a thread from the pinned section.` | Switch | | false | `confirmThreadUnpin` |
| | Archive confirmation | `Require a second click on the inline archive action before a thread is archived.` | Switch | | false | `confirmThreadArchive` |
| | Delete confirmation | `Ask before deleting a thread and its chat history.` | Switch | | true | `confirmThreadDelete` |
| | Terminal close confirmation | `Ask before closing terminals and stopping their running processes.` | Switch | | true | `confirmTerminalClose` |
| | Quit shortcut (desktop only) | `Hold mode also quits on two quick presses.` | Select 160px | `Direct`=direct, `Hold`=hold, `Double press`=double-click | hold | Cmd/Ctrl+Q behavior. `confirmQuit` |
| Display | Language | `System default follows your browser or OS language.` | Select | | `en` | Drop in Tern (English only) |
| | Time format | `System default follows your browser or OS clock preference.` | Select 160px | `System default`=locale, `12-hour`, `24-hour` | locale | all timestamps. `timestampFormat` |

Every row has a reset button when not default (aria e.g. `Reset project order to default`).
Notification permission errors replace the description:
`Allow notifications in your browser or system settings, then choose this option again. Sound only is still available.`
(Tern: show this if the OS denies notification permission.)

---

## 3. Chat (`SettingsPanels.tsx:1233-1637`)

Device-only page. `{mod}` = `⌘` on macOS, `Ctrl` elsewhere.

| Section | Title | Description | Control | Options | Default | Affects |
|---|---|---|---|---|---|---|
| Composer | Rich text composer | `Show formatted Markdown as you type.` | Switch | | true | composer live markdown. `composerRichTextEnabled` |
| | Send shortcut | `Choose when Enter sends a prompt or inserts a new line` (no period) | Select, auto width; items show a 16px `Check` on the current one | `Enter`=enter, `{mod} + Enter for multiline prompts`=mod-enter-multiline, `{mod} + Enter always`=mod-enter | enter | composer submit key. `sendShortcut` |
| | Advanced > Show skills in slash menu | `Also include skills in the / command menu. Skills always appear when you type $.` | Switch | | true | `showSkillsInSlashMenu` |
| Follow-ups | Follow-up behavior | depends on sendShortcut, see below | Select auto width | `Queue`=queue, `Steer`=steer | steer | sending while a turn runs. `followUpBehavior` |
| Subagents and side chats | Open subagents and side chats in | panel: `Where a click opens a subagent, a side chat, or a thread on a linked environment. {mod}-click or middle-click opens it in the main view instead.` main: same with `...opens it in the side panel instead.` | Select auto width | `Side panel`=panel, `Main view`=main | panel | `sideThreadOpenTarget`. Drop "or a thread on a linked environment" |
| Activity | Tool calls in chat | `Hide the commands, edits, and searches agents run. Hide successful keeps failures. Zen shows only final answers. Approvals, questions, and errors always show.` | Select 160px | `Show all`=showAll, `Hide successful`=hideSuccessful, `Hide all`=hideAll, `Zen`=zen | showAll | timeline filtering. `toolCallVisibility` |
| Screen reader | Screen reader announcements | `What a screen reader hears from the open thread: a turn starting, needing you, and ending. Status and messages adds what the agent writes before its last message. Everything adds each step.` | Select 192px | `Status only`=status, `Status and messages`=messages, `Everything`=everything | status | `screenReaderAnnouncements` |
| | Screen reader window title | `Start the window title with the thread's status, like Needs approval, and its name, so screen readers read them first when you switch apps. Taskbars, shared screens, and time trackers see it too.` | Switch | | false | window title format. `screenReaderWindowTitle` |

Follow-up behavior description:
- enter: `Queue follow-ups while the agent runs or steer the current run. Press {mod} + Enter to do the opposite for one message.`
- mod-enter-multiline: `Queue follow-ups while the agent runs or steer the current run. Press {mod} + Enter for single-line prompts or {mod} + Shift + Enter for multiline prompts to do the opposite for one message.`
- mod-enter: `Queue follow-ups while the agent runs or steer the current run. Press {mod} + Shift + Enter to do the opposite for one message.`

---

## 4. Diffs & pull requests (`SettingsPanels.tsx:1639-1853`, `IntegrationsSettings.tsx:843-875`)

Device-only page.

| Section | Title | Description | Control | Options | Default | Affects |
|---|---|---|---|---|---|---|
| Diffs | Diff layout | `Show diffs stacked or side by side. The toggle in the diff toolbar changes this too.` | Select 160px | `Stacked`=stacked, `Split`=split | stacked | `diffLayout` |
| | Hide whitespace changes | `Set whether the diff panel ignores whitespace-only edits by default.` | Switch | | true | `diffIgnoreWhitespace` |
| | Default diff file state | `Start with files expanded or collapsed when opening diffs or a pull request's Code tab.` | Select 160px | `Expanded` -> false, `Collapsed` -> true | collapsed (true) | `diffFilesCollapsed` (bool) |
| | Diff colors | `Choose colors for additions and deletions, including change counts.` | Select 160px; trigger shows two 8px dots (`bg-diff-deletion`, `bg-diff-addition`, gap 4px; order reversed for blue-orange) then the label | `Red & green (default)`=red-green (trigger label `Red & green`), `Blue & orange`=blue-orange | red-green | diff palette tokens. `diffColorScheme` |
| Pull requests | Open pull requests and changes panels proactively | `Open linked pull requests first. Otherwise, open Changes for edits to at least 3 files or 50 lines.` | Switch | | false | auto-open right panel. `proactivePanelsEnabled` |
| | Open pull requests in T3 Code (rename "in Tern") | `Clicking a pull request link opens it in T3 Code. Turn off to open pull request links like any other link, on your git host.` | Switch | | true | PR link routing. `pullRequestLinksOpenInApp` |

---

## 5. Threads (`SettingsPanels.tsx:1935-2377`)

Has the scope sentence. Keys are server settings (section 16); rows marked P accept project
overrides.

| Section | Title | Description | Control | Default | Key |
|---|---|---|---|---|---|
| New projects | Add project starts in | `Leave empty to use "~/" when the Add Project browser opens.` | Text input 288px, placeholder `~/`, commits on blur/Enter, pasted paths normalized | `""` | `addProjectBaseDirectory` |
| | Clone into host folders | `Clones go to github.com/owner/repo under the folder above, like ghq.` | Switch | false | `ghqCloneLayout` |
| (Project defaults) | rendered by `<ProjectDefaultsSettings category="general" />`, see section 12.3 | | | | |
| Snooze (hidden in project scope) | Snooze options | `Choices in every snooze menu, listed soonest first. Weekdays mean the next one after today.` | list editor, below | 5 presets | `snoozePresets` |
| Usage limits | Resume after usage limits | `Automatically resume threads when their usage limits reset. You can cancel auto-resume in each thread.` | Switch | false | `autoResumeLimitedThreads` P |
| | Snooze until limits reset | `Snooze threads until their usage limits reset. Enable auto-resume to continue the agent after the reset.` | Switch | false | `snoozeLimitedThreads` P |
| Auto-settle | Auto-settle merged threads | `Settle a thread when its pull request merges. Closed pull requests still settle automatically.` | Switch | true | `sidebarAutoSettleOnMerge` |
| | Auto-settle inactive threads | `Sidebar threads with no activity for this long settle automatically.` | Switch (on writes 7, off writes null) | 7 | `sidebarAutoSettleAfterDays` |
| | Days of inactivity before auto-settle (only when on) | `Any new activity un-settles a thread automatically.` | Number input 96px, min 1 max 90, commit on blur/Enter, Esc reverts | 7 | same |
| | Keep threads with a PR (only when on) | `Inactivity does not settle threads with a pull request. Merge and close still do.` | Switch | false | `sidebarAutoSettleKeepPullRequests` |
| Text generation | Text generation model | `For thread titles and other generated text. Source control has its own setting. Text generators from Providers also appear here.` | provider/model picker + traits picker (composer controls restyled with `SETTINGS_PICKER_TRIGGER_CLASSNAME`: `border-input bg-background shadow-xs`, chevron stroke 1.75 at 70%), gap 6px, wrap | `{instanceId:"codex", model:"gpt-6-luna", options:[{id:"reasoningEffort",value:"low"}]}` | `textGenerationModelSelection` P |
| | Advanced > Thread title instructions | `Appended to the prompt for new and regenerated thread titles.` | Textarea below the row (margin-top 12px, max-width 672px, padding-bottom 14px), 4 rows, placeholder `Keep titles to a few words. Lead with the main subject.` | `""` | `threadTitleInstructions` P |
| | Advanced > Response streaming | turn: `Text appears once the agent finishes its turn.` paragraph: `Each paragraph or code block appears as soon as it is complete.` | Select 224px: `Wait for the full response`=turn, `Show finished paragraphs`=paragraph | paragraph | `responseStreamingMode` P |

Text generation empty states (in the control slot, 14px muted):
`No text generation providers available.`; a disallowed pick toasts
`Text generation model not saved` with the reason.

Snooze options editor (`SnoozePresetsSettings.tsx:200-520`): rendered as the row's children,
column gap 6px, padding 12px 0 8px. Each preset is a wrapping row (gap 8px):
- `When` select 128px: `In` (=after), `Today`, `Tomorrow`, `Monday`..`Sunday` (locale weekday names).
- after: number stepper 128px (min 1, step 1, commits on blur/Enter/step) + unit select 112px
  `minute(s)` / `hour(s)` / `day(s)` (plural follows amount). Max total 30 days (43200 min).
- at: 12px muted `at` + native time input 128px (`HH:MM`).
- Right (margin-left auto, 12px muted): preview `<label>` (foreground) + 8px + `<when>` tabular, or
  one of `Passed for today`, `Duplicate of an option above`, `Same time as an option above`; then a
  ghost-muted `icon-sm` `X` (aria `Remove snooze option {n}`).
- Footer line 12px muted: `Menu now: a · b · c` or `Menus offer only Custom…`.
- `Add option` (ghost-muted compact, `Plus` icon); at 8 presets it is aria-disabled and shows
  `Up to 8 options`. Rows re-sort soonest-first when focus leaves the list.
- Default presets (`packages/contracts/src/settings.ts:126-132`): after 60, after 180,
  at today 18:00, at tomorrow 09:00, at monday 09:00.

Compact idle threads row (`SettingsPanels.tsx:1855-1933`) lives on the Providers page: title
`Compact idle threads`, description `When to compact an idle Claude thread over 200,000 tokens:
after you return, just before its prompt cache expires, or never. Each Claude provider can
override this.`, select 224px: `When you return`=onReturn, `Before the cache expires (beta)`=
beforeCacheExpires, `Never`=off. Default onReturn. Writes `idleCompaction` and the legacy
`compactIdleThreads` (false for onReturn, true for beforeCacheExpires, "off" string for off;
`settings.ts:1728-1745`).

---

## 6. Maintenance (`SettingsPanels.tsx:2379-2524`, `BackgroundActivitySettings.tsx`, `StorageSettings.tsx:402-972`)

Scope sentence shown. Server keys.

| Section | Title | Description | Control | Default | Key |
|---|---|---|---|---|---|
| Updates | Provider update checks | `Check installed provider CLIs for newer available versions and fetch model catalog updates. When off, new models may not appear.` | Switch | true | `enableProviderUpdateChecks` |
| | Continue threads after restarts | `Automatically resume interrupted threads after an update, crash, or machine restart on the selected environments.` (Tern: end at "restart.") | Switch | true | `continueThreadsAfterServerUpdate` |
| Background activity | Background activity (+ policy tooltip: `This shared policy gates background work such as Git refreshes and provider health probes after their individual intervals elapse.`) | per profile: balanced `Pauses probes for idle clients, locked hosts, or low power mode.` / performance `Allows scoped background probes while any subscribed client remains connected.` / battery-saver `Also pauses background probes when the host or client is on battery.` / advanced `Uses custom intervals. Shared policy: {profile}.` | Select 160px: `Balanced`, `Performance`, `Battery saver`, `Advanced` (opens dialog) | balanced | `backgroundActivity`, `backgroundActivityProfile` |
| | Keep awake while agents work | `Stop the computer from going to sleep while agents are working, and let it sleep once they finish. Closing a laptop lid still puts it to sleep.` | Switch | true | `keepAwakeWhileAgentsWork` |
| Artifacts and logs | Delete old rotated logs | `Delete inactive rotated log files after this many days. Current logs are kept.` | Retention control | null (off) | `storageCleanup.logsAfterDays` |
| | Delete old browser artifacts | browser feature, drop | | | |
| Cleanup (plain) | Delete now | `Turn on a cleanup rule to use this.` or `Last run {when}. {bytes} recovered.` / `Cleanup hasn't run yet.` / `Loading last run…` / `Could not load the last run.` | outline sm button `Delete now` (`Deleting…` while running) | | runs cleanup |
| | Last run (folded section, only after a run) | summary `{removed} removed, {kept} kept` (+ `, {failed} failed`) | table of removed items | | |
| Diagnostics | Diagnostics | dropped page; drop the section | | | |

Advanced background dialog (`BackgroundActivitySettings.tsx:115-340`, max-width 576px): title
`Background Activity`, description `Tune the shared power policy and the background intervals that
feed it.` A bordered card (radius 12px, `bg-card`) of rows (padding 12px 16px, separated by
borders): `Shared policy` (`Controls whether background work may run after a subscribed interval
fires.`, select 160px), `Git fetch interval` (`Refresh remote branch status in the background.`,
seconds stepper, default 30s, key `automaticGitFetchInterval` stored as ms), `Provider health
interval` (`Refresh provider availability, versions, and auth state. Model catalog updates follow
Provider update checks.`, default 300s, key `providerHealthRefreshInterval` ms), `Host power
monitor`, `Idle host monitor`, then a 2-column grid of switches `Pause when host is locked`,
`Pause on host low power`, `Pause on client low power`, `Pause on battery`. Footer: outline
`Reset all`, primary `Done`. In Tern keep the two intervals and the four pause switches; drop the
client/host split wording if there is only one process (open question).

Retention control (`StorageSettings.tsx:118-262`): off state shows 12px muted `Off` then a switch.
Turning on shows a number stepper (min 1, max 3650, draft starts at 8, `days` suffix 12px) left of
the switch; the value saves on Enter or when focus leaves the control, never mid-typing. Esc
reverts an unsaved draft without closing settings.

---

## 7. About (`SettingsPanels.tsx:2526-2617`, `AboutVersionTitle.tsx`, `OpenSourceLicenses.tsx`)

Device-only page.

| Section | Title | Description | Control |
|---|---|---|---|
| App | `Version` + version string as an inline muted button in 11px `font-medium` mono; click copies (tooltip `Copy version`; failure toast `Could not copy version`) | `Current version of the application.` | none in Tern (updater dropped) |
| | t3 command | CLI install row; drop unless Tern ships a CLI | |
| Legal | Privacy policy | drop (no telemetry) | |
| | Open source licenses | `Notices for dependencies, assets, and optional tools used by T3 Code.` (say Tern) | outline sm `View licenses` -> `/settings/open-source-licenses` |

Dropped rows: Organization (managed policy), Update track, Mobile app, Welcome page.

### 7.1 Open source licenses page

One section titled `Third-party notices`. Header action: 12px muted tabular count
`{n} notices` (or `{filtered} of {total}` while filtering) + ghost-muted `icon-micro` search
button (`Search` 12px, tooltip `Search licenses`). Clicking it swaps to a 176px input group with a
12px search icon, placeholder `Search licenses`, autofocus; Esc clears and closes it, blur with
empty query closes it. Filter matches name, version, license, bundle labels (case-insensitive).

Rows (one open at a time): min-height 36px, hover `bg-muted` 35%. Trigger padding-x 16px, gap
10px: `ChevronRight` 14px muted (rotates 90deg, 200ms), name 14px `font-medium` truncate, version
12px muted mono, then right-aligned (max 42% width) 12px muted `{license} · {bundles}`. Optional
ghost-muted `icon-micro` external-link button (12px icon, margin-right 16px, title
`Project source`). Expanded panel: padding 4px 40px 16px, `pre` 12px/20px mono
`text-foreground` 80%, wraps, max 76ch.

Empty search: `No licenses match that search.` (14px muted centered, padding 32px 16px).
Loading: `Loading open-source notices…`. Error: `Open-source notices are unavailable` + message +
outline xs `Try again`.

Data: `third-party-licenses.json` `{schemaVersion: 1, entries: [{name, version|null, license,
kind: "package"|"custom", bundles: string[], noticeText, sourceUrl|null}]}`
(`packages/shared/src/thirdPartyLicenses.ts:4-17`). Tern should generate the same shape from
`cargo about` (or similar) at build time and embed it. Bundle labels: `Desktop`, `Assets`, ...

---

## 8. Archive (`ArchivedThreadsPanel.tsx`)

Has the scope sentence (filters by project).

- Empty / loading: one section titled `Archived threads` with one row: title prefixed by a 14px
  muted `Archive` icon (or a small spinner) and text `No archived threads` / `Loading archived
  threads` / `Could not load archived threads`; description `Archived threads will appear here.`
  (loading: `Checking connected environments.`).
- Otherwise one section per project: title = project name, icon = project favicon (16px). Rows
  sorted newest `archivedAt` first (fallback `createdAt`).
- Row: title = thread title; description `Archived {relative} · Created {relative}`; control: two
  `icon-sm` (28px) buttons, gap 8px, 14px icons, with tooltips: `Unarchive` (outline, `ArchiveX`
  icon) and `Delete` (destructive-outline, `Trash2`). Right-click opens a native context menu
  with `Unarchive` and `Delete` (destructive). Buttons disable while that thread's action runs.
  Delete goes through the normal delete confirmation (`confirmThreadDelete`).
  Failure toasts: `Failed to unarchive thread`, `Failed to delete thread`.
- Rows mount in chunks of 50 near the viewport (estimate 67px per row). In GPUI use a
  `uniform_list`/`list` instead.

---

## 9. Appearance (`AppearanceSettingsPanel.tsx`, `ThemeSettings.tsx`, `TypographySettings.tsx`)

Device-only page. Three sections, 32px apart.

### 9.1 Colors & themes (section `appearance`, title hidden, `plain` variant)

**Color scheme** heading, then a 3-column grid (gap 12px) of tiles in order `System`, `Light`,
`Dark` (`ThemeSettings.tsx:734-772`):
- Tile: column, gap 6px, radius 12px, 1px border, padding 8px. Inactive: `border` 70%, `bg-card`
  60%, hover `bg-accent` 10%, label `text-muted-foreground`. Active: transparent border,
  `bg-accent` 30%, 2px inset `ring`, label `text-foreground`. Label 12px `font-medium`, centered.
- Each tile holds a 140px-high wireframe (radius 8px, 1px `border` 60%). System splits it: left
  half painted with the light theme, right half with the dark one, 2px seam
  (`ThemeWireframe.tsx`). Wireframe pieces (percent of tile; line color `rgb(127 127 127/.25)`):

| Element | Color role | Box | Notes |
|---|---|---|---|
| Canvas | canvas | full | |
| Sidebar | sidebar | left, 22% wide | inset right border |
| Sidebar item 1 | surface | left 3%, top 8%, 16% x 8% | radius 6px, 1px line ring |
| Sidebar items 2-4 | accentSurface; messageSurface @0.7; @0.5 | top 22% / 32% / 42%, 7% high | radius 6px |
| User bubble | messageSurface | right 28%, top 11%, 24% x 9% | radius 8px |
| Text lines | line color | left 27%, top 28% (34% wide) and 38% (26% wide), 5% high | |
| Composer | surface | bottom 8%, left 26%, right 6%, 15% high | radius 6px; pill + round messageAction send dot |
| Floating panel | surface | right 5%, top 8%, 20% x 46% | radius 8px, shadow `0 2px 5px rgb(0 0 0/.14)`, three rows with dots `#34d399`, messageAction, `#fbbf24` at 0.55 |

Mode is stored outside settings (9.5). `system` follows the OS appearance.

**Themes** header (`ThemeSettings.tsx:903-926`): min-height 32px, padding-top 20px, padding-x
16px; title `Themes` left; right two outline xs buttons `Create theme` (Paintbrush) and
`Add theme` (Plus).

Grid: gap 8px, `repeat(auto-fit, minmax(min(100%, 256px), 1fr))`. Order: stock `T3 Code` card
(id `default`; selecting it stores the bare mode string), then built-ins, then custom themes.

| id | Label | Base | Variants | Sidebar artwork | Accent light / dark |
|---|---|---|---|---|---|
| default | T3 Code (Tern: rename) | light+dark | | | `#1b4ed8` / `#346bf1` |
| t3-chat | T3 Chat | light | dark | yes | `oklch(0.5916 0.218 0.58)` / `oklch(0.4607 0.185 4.1)` |
| grove | Grove | light | dark | yes | `packages/shared/src/themePalettes.ts:373-505` |
| ocean | Ocean | light | dark | yes | `:508-640` |
| ember | Ember | light | dark | yes | `:643-775` |
| iris | Iris | light | dark | yes | `:778-910` |

Legacy ids `t3-chat-dark, t3-grove, t3-ocean, t3-ember, t3-iris` alias to the above
(`themePalette.ts:409-415`). Palettes (60 roles each, `THEME_COLOR_ROLES`,
`themePalettes.ts:30-92`) are copied verbatim into spec/01.

Theme card (`ThemeSettings.tsx:147-477`, `ThemePreviewCircles.tsx`):
- Container radius 12px, 1px `border` 70%, `bg-card` 60%, hover `bg-accent` 10%; whole card
  clickable (applies to both modes). Tooltip `Use for both light and dark` / `Use for light mode
  only` / `Use for dark mode only`.
- Preview row: min-height 64px, gap 10px, padding 12px 12px 0. One 68px circle button per mode
  (6px padding, hover scale 1.05). Picked: 2px inset `ring` + 20px badge bottom-right (1px
  `border` 70%, `bg-background`, 12px Sun or Moon). Clicking a circle assigns that theme to that
  mode only ("halves").
- Circle: 56px, 2px `background` border; fill = canvas mixed 80% (oklab) with `#ffffff` (light)
  or `#09090b` (dark); two radial gradients: light: accent at (72%,22%) 100% -> 72% @28% ->
  transparent @58%, messageAction at (18%,82%) 45% -> transparent @55%; dark: accent at
  (28%,78%) mid 62%, action at (82%,18%). Blur 3px, scale 1.1. Edge shadow light
  `inset 0 0 0 1px rgb(0 0 0/.10), 0 1px 2px rgb(0 0 0/.08)`, dark
  `inset 0 0 0 1px rgb(255 255 255/.14), 0 1px 2px rgb(0 0 0/.18)`.
- Footer: gap 8px, padding 8px 12px 12px; name 14px `font-medium` truncate; xs ghost icon
  buttons (gap 4px): `Duplicate theme` (all), `Edit theme`, `Export theme file`, `Remove theme`
  (custom only; remove is destructive).
- Collection cards (custom themes sharing `collection.id`): 80px preview, light/dark root circles
  offset +-52px; hover fans variants as 28px circles (200ms ease-out, 35ms stagger). Optional;
  low priority.

Add theme dialog (`ThemeImportDialog.tsx`, max 768px): Open VSX community search (network;
open question whether Tern keeps it), divider `or import a file`, dashed drop zone `Theme file` /
`Drop T3 Code or VS Code .json files` / `Choose files` (256KB per file), `Theme JSON` editor
(min-height 176px), buttons `Cancel` / `Add theme`. Id collision: `Already installed` with
`Update existing`, `Keep both`, `Back`, `Cancel`. Remove confirm: `Remove “{label}”?` /
`You can bring it back anytime by importing its JSON file.`

Theme file format (`themePalette.ts:64-74,1536-1622`):

```json
{ "version": 1, "id": "optional-kebab", "name": "<=48 chars",
  "appearance": "light" | "dark",
  "colors": { "<role>": "<css color>" },
  "variants": { "dark": { "<role>": "<css color>" } },
  "collection": { "id": "...", "label": "..." },
  "canvas": "...", "accent": "...",
  "css": "<=100000 chars" }
```

Id `^[a-z0-9][a-z0-9-]{0,47}$` (derived from name if missing). Missing roles fall back to the
stock palette of that appearance. VS Code themes (dotted color keys or `tokenColors`) are
converted (`vscodeThemeImport.ts`). Export writes `{id}.json`, 2-space indent. Theme editor panel
(`ThemeEditorPanel.tsx`, floating 416 x <=672px): `Create theme` / `Edit theme`, `Theme name`
(placeholder `e.g. Aurora`), Appearance Light/Dark, Colors (`Two colors, rest derived`: canvas +
accent; Advanced switch shows all role families), `Custom CSS` (drop in Tern), footer `Cancel` /
`Create theme` or `Save changes`. Phase 2.

Theme -> tokens: each role becomes a token override; `primary` = messageAction, `accent` =
accentSurface, `background` = canvas, `card` = surface, `popover` = surfaceOverlay, `ring` =
focus (`index.css:1358-1415`).

### 9.2 Interface (section `appearance-interface`)

| Title | Description | Control | Options / range | Default | Key / effect |
|---|---|---|---|---|---|
| Contrast | `Adjust the contrast of colors and borders across the interface.` | slider + `{n}%` readout | 50-200 step 5 | 100 | `appearanceContrast`; base = min(c,100)%, boost = max(c-100,0)%, border boost = boost/4 (`appearanceContrast.ts`) |
| Glass opacity | `Higher values make menus, dialogs, and the composer more solid.` | slider + `{n}%` | 40-100 step 5 | 80 | `glassOpacity`; at 100 blur is 0 |
| Chat width | `Set how wide messages and the composer can grow on large screens.` | select 160px | `Comfortable (default)`=comfortable (trigger shows `Comfortable`), `Wide`=wide, `Full`=full | comfortable | `chatWidth`; max 736px / 1152px / 100% |
| HTML preview width | `Let HTML previews in a chat grow wider than its messages, which stay centered.` | select 160px | `Match chat (default)`=chat, `Wide`, `Full` | chat | `htmlPreviewWidth`. Drop with HTML render frames |
| Thread details | `Choose what the thread details card shows. You can also right-click the card.` | outline button `{enabled} of {total} on` + chevron, 160px, opens checkbox menu | Environment and worktree, Open in editor, Project scripts, Branch and pull request, Git actions, Changes, Servers, Automations, Lineage, Side chats (on); Usage limits, Context window (off) | as listed | `threadDetailsItems` sparse `{id: bool}` |
| Effort display | `Show reasoning effort in the composer as its name, such as High, or as bars.` | select | `Word (default)`=label, `Bars`=bars | label | `effortDisplay` |
| Render math | `Typeset LaTeX math, such as $x^2$ or \[x^2\], in messages and Markdown previews. The composer keeps TeX source editable.` | switch | | false | `mathRenderingEnabled` |
| Advanced > Panel animations | `Set how fast panels open and close.` | clickable preview + slider + `{n} ms` | 0-400 step 25 | 0 | `panelAnimationDurationMs`; 0 = no animation; reduced-motion also disables |
| Advanced > Environment identification | `Show Dev or Nightly artwork in the sidebar header.` (varies by build channel) | select 160px | `None`=artwork (stable), `Nightly artwork`, `Dev artwork`, `Version pill`=pill, `None`=none | artwork | `environmentIdentificationMode`. Tern has no channels: drop (open question) |
| Advanced > Composer context | `Keep branch and worktree controls below the composer after a thread starts.` | switch | | false | `persistComposerContextStrip` |

Slider: track + readout pill in a 208px flex row, gap 12px; readout min-width 48px (64px for
ms), padding 4px 8px, `bg-muted`, radius 6px, 12px mono tabular. Panel animations preview
(`PanelAnimationsPreview.tsx`): 40px-high button, radius 8px, `border`, `bg-background`, padding
4px: sidebar block (`sidebar`, 16px wide), three 2px lines (`muted-foreground` 25/20/15%), an 8px
bottom strip, right panel block (`muted`, 20px); click collapses side blocks to 0 over the chosen
duration, ease-out. Layout grid `112px | 208px`, gap 16px.

### 9.3 Typography (section `typography`, `TypographySettings.tsx`)

Simple mode (default, `Separate fonts` off):

| Title | Description | Preview below row |
|---|---|---|
| Interface font | `Everything outside code blocks and the terminal.` | prompt preview |
| Monospace font | `Code blocks, diffs, file previews, and the terminal.` | diff preview + terminal preview |

Separate fonts on:

| Title | Description | Family key | Size key, range, default |
|---|---|---|---|
| Interface font | `Everything outside code blocks and the terminal.` | `fontFamilySans` | `fontSizeInterface` 12-20, 16 |
| Prompt font | `Only the box you write prompts in. Mono works well here.` | `fontFamilyComposer` | `fontSizePrompt` 12-20, 14 |
| Code font | `Code blocks, diffs, and file previews.` | `fontFamilyCode` (monospace only) | `fontSizeCode` 10-18, 13 |
| Terminal font | `Terminal output, independent from code blocks and diffs.` | `fontFamilyTerminal` (monospace only) | `fontSizeTerminal` 8-20, 12 |

Then: `Word wrap` (`Wrap long lines in code blocks, diffs, and file previews by default.`, switch,
`wordWrap` true); Advanced fold: `Separate fonts` (`Set the prompt, code, and terminal fonts
separately instead of one monospace font.`, switch, device flag, default off), `Font smoothing`
(macOS only: `Use thinner grayscale text smoothing instead of the macOS default.`, `fontSmoothing`
true).

Font row control: family picker (min 176px) + size select 88px listing every integer `{n} px`.
Reset resets family and size. Picker: searchable combobox (popup 288px, placeholder
`Search fonts…`, empty `No fonts found.`), first row the default family tagged `default`, each
row drawn in its own font. Non-monospace pick for code/terminal is refused with toast
`"{font}" isn't monospace` / `Code and terminal need a fixed-width font, so the current font was
kept.` Families default `""` = system stack (sans `-apple-system, BlinkMacSystemFont, "Inter
Variable", "Segoe UI", system-ui, sans-serif`; mono `"SF Mono", "SFMono-Regular", Menlo,
Consolas, "Liberation Mono", monospace`). In GPUI enumerate fonts with the text system and test
monospace by comparing advance widths of `i` and `W`.

Effects: interface size sets the root rem (all 16px-based measurements scale with it); prompt
size applies to the composer only in separate mode (else 14px); code size drives code blocks and
diffs; terminal size the terminal.

Previews (`SettingsFontPreviews.tsx`, margin 4px 0 8px, radius 8px, `border`):
- Prompt: `Use $frontend-design to fix the flaky test in [surface.test.ts](apps/web/src/terminal/ghostty/surface.test.ts) and align the header with [SettingsPanels.tsx](apps/web/src/components/settings/SettingsPanels.tsx) before shipping.` (skill chip + file chips, editable).
- Code: unified diff of `src/formatUser.ts`: `-  return user.name.toUpperCase();` / `+  return `${user.name} <${user.email}>`; // 0O 1lI`.
- Terminal (208px high): `→ t3code git:(main) ✗ vpr dev`, `VITE v7.1.1  ready in 1.24s`,
  `→  Local:    http://127.0.0.1:5173/`, `→  Network:  http://192.168.1.24:5173/`,
  `✓ 85 passed   △ 2 warnings   ✗ 0 failed`, ` READY  watching for changes, press q to quit`.

### 9.4 Restore

All of the above reset via the General header's Restore device defaults (1.2).

### 9.5 Theme storage (device-local, not in client settings)

| Key (T3 localStorage) | Value | Default |
|---|---|---|
| `t3code:theme` | `system` / `light` / `dark` / theme id | system |
| `t3code:theme-appearance-mode` | `system` / `light` / `dark` | system |
| `t3code:theme-halves:v1` | `{"light"?: id, "dark"?: id}` | absent |
| `t3code:themes:v1` | array of theme files | [] |
| `t3code:typography-advanced` | bool (Separate fonts) | false |

Tern stores these in `ui-state.json` (section 16) under the same names minus the prefix, and
custom themes as files in `<stateDir>/themes/*.json` (T3's server already reads that folder for
published themes, `apps/server/src/config.ts:164`).

---

## 10. Providers (`ProviderSettingsPanel.tsx` = PSP, `ProviderInstanceCard.tsx` = PIC, `ProviderSettingsForm.tsx` = PSF)

Scope sentence shown (single-environment). Section gap 32px. Tern drivers: Codex and Claude
(spec/07). Everything for other drivers is listed for completeness at 10.6.

### 10.1 Providers section (PSP:1025-1145)

`SettingsSection` `Providers`, `plain` variant.
- Title action: ghost-muted `icon-xs` (24px) `Plus`, tooltip/aria `Add provider`.
- Header action (gap 8px, margin-right -8px): refresh button, ghost-muted xs (24px), spinning
  `RefreshCw`-style icon while refreshing; label (hidden < 640px) `Refreshing providers` or
  `Checked {relative} ago` / `Checked {time}`; tooltip `Refresh provider status`. Refreshes all
  probes including model lists. (`Update all` provider-update action: keep only if Tern runs CLI
  updates; open question.)
- Master/detail card: height `min(704px, 100vh - 176px)`, min 512px, when the container is at
  least 768px; grid `272px | 1fr`. Below 768px the panes stack.
  - Left list: `bg-muted` 10%, `border-r` (`border-b` when stacked), rows divided by `border` 50%.
    One list row per instance, then an `Add provider` row (padding 12px 16px, gap 12px, 14px
    `text-muted-foreground`, 16px `Plus`, hover `bg-muted` 25%).
  - Right detail: scroll area, padding-y 16px, sections 24px apart: the selected instance in
    editor mode. Empty: `No providers configured.` (padding 24px, 14px muted).
- Rows listed: the default slot of `codex` and `claudeAgent` always; any other instance with
  stored config; non-default instances grouped after their driver's default. First row selected.

List row (PIC:1173-1255): min-height 72px, padding 12px 16px, gap 12px. Selected
`bg-foreground` 6%, else hover `bg-muted` 25%. Disabled + unselected: 60% opacity (100% on
hover). Whole row selects (aria `Select {provider}`).
- Icon box 20px (16px glyph, `text-foreground` 80%); accent color shows as a 12px badge at
  bottom-right, offset -2px.
- Line 1 (gap 8px): name 14px `font-medium` truncate; version 12px muted mono (max 96px, `v`
  prefix when it starts with a digit); version advisory `icon-micro` button when relevant.
- Line 2 (margin-top 2px, gap 6px, 12px `text-muted-foreground` 80%, 2-line clamp): 6px status
  dot only for warning/error, then the headline (+ ` · {detail}` when it needs attention).
  While updating: `Updating · {progress}`.
- Right (20px high, gap 4px): enable `Switch` (aria `Enable {provider}`, writes
  `providerInstances[id].enabled`). In project scope also a `Layers` icon (`text-primary`,
  tooltip `Overridden for this project`) and a reset (`Reset to the device setting`).

Status dot colors (`providerStatus.ts:10-26`): disabled `muted-foreground` 50%, error
`destructive`, neutral `muted-foreground`, ready `success`, warning `warning`.

Headline mapping (`providerStatus.ts:59-127`), first match wins:

| Condition | Headline | Default detail |
|---|---|---|
| no probe yet | `Checking provider status` | `Waiting for the server to report installation and authentication details.` |
| disabled | `Disabled` | `This provider is installed but disabled for new sessions in T3 Code.` |
| not installed | `Not found` | `CLI not detected on PATH.` |
| unauthenticated | `Not authenticated · {method}` / `Not authenticated` | probe message |
| status warning | `Needs attention` | `The provider is installed, but the server could not fully verify it.` |
| status error | `Unavailable` | `The provider failed its startup checks.` |
| authenticated | `Authenticated · {method}` / `Authenticated` | probe message |
| else | `Available` | probe message |

### 10.2 Editor (detail pane, PIC:1258-1516)

Sections in order, each a `SettingsSection`:

1. **Header** section titled with the display name + icon. Header actions (gap 6px): version
   (12px muted mono), advisory button, reset (`Undo2`, default slot with stored config only, aria
   `Reset {provider} provider settings to default`), delete (`icon-xs` ghost-destructive
   `Trash2`, non-default instances only, aria `Delete instance {instanceId}`). Rows:
   - `Status`: `Authenticated as {email}` (email can be masked; click toggles, tooltips
     `Click to reveal email` / `Click to hide email`) + ` · {authLabel}` + ` · {detail}`;
     otherwise dot + headline + ` · {detail}`.
   - `Display name`: accent picker (28px `icon-sm` filled with the color, or `Plus`; popover
     224px HSV + hex; `Add color`, `Change accent color`, `Clear color`; `#RRGGBB` only; picker
     fallback `#2563eb`) + text input 224px, placeholder = driver label. Empty removes
     `displayName`.
2. **Runtime** section: the driver form (10.3).
3. **Environment** section, row `Variables`: `API keys, base URLs, and other per-instance CLI
   settings.`; outline sm `Add variable` (`Plus`). Each variable: bordered card (radius 8px,
   padding 8px, gap 6px): name input (mono, placeholder `VARIABLE_NAME`, must match
   `^[a-zA-Z_][a-zA-Z0-9_]*$`, max 128), remove `icon-micro` `X`, value input (placeholder
   `value`), lock toggle (`Lock`/`LockOpen`, tooltips `Sensitive, stored separately` /
   `Plain text`; new rows default sensitive). Footer `Sensitive values are stored separately and
   never returned to the app.` Tern: store sensitive values in the OS keychain (keyring crate)
   keyed by instance+name; keep the flag in JSON (open question).
4. **Models** section (10.4), caption 12px muted: `Model preferences and custom models are saved
   on the selected environment and shared across connected devices.` (Tern: drop the caption or
   say "saved in settings").

Delete confirm: `Remove the {provider} provider instance?` + newline + `Its saved settings,
including any custom binary path, will be removed.` Reset confirm: `Reset the {provider} provider
to its defaults?` (+ ` It will also be turned off.`). Toasts: `Could not update provider
instance` / `The settings update failed.`, `Could not delete provider instance`, `Could not reset
provider instance`.

Version advisory popover (PIC:1055-1171): titles `Update available` / `Limited support` /
`Unsupported version` / `Known broken version`; details `Use {version} for full support.` /
`Update for full support.` / `Update available: install {version}.`; then `or, update manually
using` + the command in mono with a copy button (`Copy command`). Version thresholds come from the
driver (spec/07).

### 10.3 Driver fields (PSF; `packages/contracts/src/settings.ts:639-871`)

Form rules: text inputs 224px; empty values with `clearWhenEmpty: "omit"` delete the key; bad
value: `Enter a valid value. It was not saved.` Commit on blur/Enter.

Codex (`codex`, label `Codex`):

| key | Label | Description | Placeholder | Default |
|---|---|---|---|---|
| binaryPath | Binary path | `Path to the Codex binary used by this instance.` | `codex` | "codex" (empty decodes to `codex`) |
| homePath | CODEX_HOME path | `Custom Codex home and config directory.` | `~/.codex` | "" |
| shadowHomePath | Shadow home path | `Account-specific Codex home. Keeps auth.json separate while sharing state from CODEX_HOME.` | `~/.codex-t3/personal` | "" |
| launchArgs | Launch arguments | `Additional CLI arguments passed to codex app-server on session start.` | | "" |

Hidden: `setupMode` (`managed`/`existing`; Tern only `existing`), `enabled` (true),
`customModels` ([]).

Claude (`claudeAgent`, label `Claude`):

| key | Label | Description | Control / placeholder | Default |
|---|---|---|---|---|
| binaryPath | Binary path | `Path to the Claude binary used by this instance.` | `claude` | "claude" |
| homePath | CLAUDE_CONFIG_DIR path | `Custom Claude home and config directory. Keeps .claude.json and .claude separate.` | `~/.claude` | "" |
| autoCompactWindow | Auto-compact after | `Compact after 100,000 to 1,000,000 tokens. Leave empty to use Claude's default.` | text, `^(?:|[1-9]\d{5}|1000000)$`, placeholder `e.g. 300000` | "" |
| idleCompaction | Compact idle threads | `When to compact an idle thread over 200,000 tokens: after you return, just before its prompt cache expires, or never. Overrides the default in Providers > Advanced.` | select `Use default`=default, `When you return`=onReturn, `Before the cache expires (beta)`=beforeCacheExpires, `Never`=off | default |
| launchArgs | Launch arguments | `Additional CLI arguments passed on session start.` | `e.g. --chrome` | "" |

Hidden: `enabled` (true), `customModels`.

### 10.4 Models (`ProviderModelsSection.tsx`, `CustomModelEditor.tsx`)

- List = probe-reported models + custom models (+ favorites no longer reported). Groups:
  `Favorites`, `All`, `Hidden from picker` (10px muted headers; first two only when favorites
  exist).
- Header: `Filter models` input 224px (only when more than 8 models); `Enable all` /
  `Disable all`; count `{n} models · {n} favorites · {n} hidden`; xs ghost-muted
  `Add custom model` (`Plus`).
- Model row: 28px, grid `[24px | 1fr | auto | minmax(88px,auto) | auto]`, gap 8px, padding-x
  8px, radius 6px, hover `bg-muted` 30%; hidden rows 50% opacity. Columns: star (filled
  `text-warning`; `Add to favorites` / `Remove from favorites`); name 12px + slug 10px mono when
  different + `custom` tag; capability labels `Fast mode`, `No reasoning`; `Move up` / `Move down`
  (within group, hidden while filtering); custom only: `Edit name and options` (`Pencil`),
  `Remove custom model`; small switch (`Shown in picker` / `Hidden from picker`; custom models
  `Custom models are always shown in the picker`). List max-height 256px, scrolls.
- Empty: `No models match.` / `No models reported for this provider yet.`
- Add: inline input + sm outline `Add` + sm ghost `Cancel`; Enter adds, Esc cancels. Placeholder
  codex `gpt-6.7-codex-ultra-preview`, claudeAgent `claude-sonnet-5`. Saved as
  `{slug, name: slug, capabilities: null}` into `config.customModels`. Errors:
  `Enter a model slug.`, `That model is already built in.`, `Model slugs must be 256 characters
  or less.`, `That custom model is already saved.`
- Custom model editor (box margin 4px 8px 8px, radius 6px, `border`, `bg-muted` 20%, padding
  12px, gap 12px): `Display name` (288px, placeholder = slug); `Options shown in the composer`
  with `Copy from…`; empty `No custom options. The composer uses the provider's default
  options.`; preset buttons + `Custom option`; option = id, label, type `Choices` / `Toggle`;
  choices value/label/`Default`; `Add choice`; `Save` / `Cancel`. Presets
  (`customModelEditor.logic.ts:49-137`): Codex `reasoningEffort` "Reasoning" (Low / Medium* /
  High / Extra High), `serviceTier` "Speed" (Standard* / Fast). Errors: `Option {n} needs an
  id.`, `Option {n}: id "{id}" is used twice.`, `Option {n} needs a label.`, `Option {n} needs at
  least one choice.`, `Option {n} has a choice without a value.`, `Option {n}: choice
  "{choiceId}" is used twice.`
- Storage: hidden/order in `providerModelPreferences[instanceId] = {hiddenModels: [], modelOrder:
  []}`; favorites in `favorites: [{provider, model}]` (client settings).

### 10.5 Advanced section (PSP:1167-1240)

- `Health check interval`: `Refresh provider status, versions, and models in the background. Set
  to 0 to disable.` Stepper min 0 step 30 + `seconds`. Default 300. Key
  `providerHealthRefreshInterval` (ms).
- `Compact idle threads` (section 5).

### 10.6 Add provider dialog (`AddProviderInstanceDialog.tsx`)

Title `Add provider`, description `Add an account or configure a provider on {environment}.`
(Tern: `Add an account or configure a provider.`). Steps `Provider -> Identity -> Config`.
- Provider: 2-column radio grid of driver cards (radius 8px, padding 12px, gap 12px, `bg-card`,
  1px ring; checked `bg-primary` 8% + 2px `primary` ring + 20px check circle). Tern: Codex,
  Claude. Multiple accounts note (codex/claude already authenticated): `We do not recommend
  managing multiple {Codex|Claude} accounts in T3 Code. More info`.
- Identity: `Label` (placeholder `e.g. Work`, `Shown in the provider list.`), `Instance ID`
  (`Letters, digits, '-', or '_'.`, placeholder `{driver}_work`, auto-derived `driver_<slug>`
  where slug is lowercase, non-alnum runs -> `_`, max 48; collisions `_2`, `_3`), `Accent
  color` (`Optional marker shown in the picker.`). Errors: `Instance ID is required.`,
  `Instance ID must be 64 characters or fewer.`, `Instance ID must start with a letter and use
  only letters, digits, '-', or '_'.`, `An instance named '{id}' already exists.` Ids `codex`
  and `claudeAgent` are reserved for the default slots.
- Config: the driver form, or `This driver has no required configuration. You can add the
  instance now.`
- Footer: `Cancel`/`Back`, `Next` / `Add instance` (`Adding...`). Success toast `Provider
  instance added` / `{provider} instance '{instanceId}' was added.`

### 10.7 Detection (what the Rust probe must produce; details in spec/07)

| Driver | Installed + version | Auth | Messages |
|---|---|---|---|
| Codex | spawn `codex app-server` (binaryPath, `CODEX_HOME`=homePath, launchArgs), JSON-RPC `initialize`; version from `userAgent` `/\/([^\s]+)/`; fallback `codex --version`; 20s timeout | `account/read`: account -> authenticated, label `OpenAI API Key` / `Amazon Bedrock` / `ChatGPT {Plan} Subscription`; `requiresOpenaiAuth` with no account -> error + unauthenticated | `Codex CLI is not authenticated. Run `codex login` and try again.`; spawn failure `Could not start Codex CLI (`{binaryPath}`). Check Settings → Providers → Codex → Binary path on the server.`; `Timed out while checking Codex app-server provider status.`; disabled `Codex is disabled in T3 Code settings.` Models from `model/list` |
| Claude | `claude --version` (20s); not found `Claude Agent CLI (`claude`) was not found on PATH.`; non-zero `Claude Agent CLI is installed but failed to run.` | init probe (25s) reads account email, subscriptionType, tokenSource, apiKeySource; unauthenticated only when tokenSource `none` and no key/email/subscription; labels `Claude API Key`, `Claude Max Subscription`, `Amazon Bedrock`, `Custom API endpoint` (custom `ANTHROPIC_BASE_URL`) | `Claude Code is not authenticated. Run `claude auth login` and try again.`; probe failure is `warning`: `Timed out after {n}s while checking Claude account status.` / `Could not verify Claude authentication status from initialization result.` |

Sign-in row (`ProviderAuthenticationSection.tsx`, title `Account`): shown when the driver can
authenticate in-app. Tern: show `Sign in` that opens a terminal running `codex login` /
`claude auth login` (the T3 terminal flow, `ProviderAuthTerminal`), and `Sign out` with confirm
`Sign out of {provider}? This stops running threads that share this sign-in. Thread history is
kept.` Descriptions: `Complete sign-in in the terminal below.`, `Signed in as {email}`,
`Checking your account…`.

### 10.8 Persisted shape (`packages/contracts/src/providerInstance.ts:25-107`)

```json
"providerInstances": {
  "codex": { "driver": "codex", "enabled": true,
             "displayName": "Codex", "accentColor": "#2563eb",
             "environment": [{ "name": "OPENAI_BASE_URL", "value": "...", "sensitive": false }],
             "config": { "binaryPath": "codex", "homePath": "", "shadowHomePath": "",
                         "launchArgs": "", "customModels": ["my-model"] } },
  "claudeAgent": { "driver": "claudeAgent", "config": { "binaryPath": "claude" } }
}
```

Instance id and driver: `^[a-zA-Z][a-zA-Z0-9_-]*$`, max 64. Default instance id = driver kind.
Enabled: off if `enabled === false` or `config.enabled === false`, else `enabled ?? config.enabled
?? true` for codex/claudeAgent (false for other drivers). `customModels` entries are a string or
`{slug, name?, capabilities?}`. Unknown drivers: keep the blob untouched and show `This instance
uses `{driver}`, which is not available in this build. Its configuration is preserved.`

Dropped here: device tabs, `Limited permissions`, Usage providers section, Text generators
section (open question), ACP registry/session management, Cursor/Grok/OpenCode/Antigravity/Kiro/
Muse/Pi/fx drivers, managed Codex download + ChatGPT OAuth (`CodexSetupSection`,
`AddCodexAccountDialog`), secret-provider references, managed policy locks.

---

## 11. Keybindings (`KeybindingsSettings.tsx` = KS, `KeybindingsSettings.logic.ts` = KSL)

Scope sentence shown (keybindings are environment-wide; in Tern hide it, open question).

### 11.1 Layout

- Section `Keybindings` (id `keybindings`). Card toolbar row (padding 12px 16px, gap 8px, wrap):
  - Search input, flex 1, 32px high, leading 14px `Search` icon, placeholder `Search keybindings`.
    Esc clears a non-empty query (stops propagation), else blurs.
  - Outline default button (32px, padding-x 11px, gap 8px) `Plus` + `Add keybinding`; disabled
    while the add row is open.
  - Outline icon button 32px `FileJson`, tooltip `Open keybindings.json`; opens the file in the
    user's editor. Failure toast `Unable to open keybindings file` / `The keybindings file was not
    opened.`
- New keybinding row (own card, row background `bg-muted` 15%): title `New keybinding`,
  description = the When control; controls (gap 8px, wrap): command combobox (trigger 224px,
  `Command` placeholder, popup 384px, opens immediately, search placeholder `Search`, empty
  `No results found.`, each item = label + right column of current shortcut Kbds), conflict icon,
  key input 176px, primary sm `Save` (`Saving`), ghost-muted `icon-sm` `X` (`Cancel`).
- Grouped sections, ids `keybindings-<group>`, fixed order:

| Title | Command prefixes |
|---|---|
| Navigation | sidebar, rightPanel, commandPalette, filePicker, projectSearch, editor |
| Threads | thread, chat, pullRequest |
| Composer | composer, modelPicker |
| Terminal | terminal |
| Preview & diff | preview, diff |
| Appearance | theme, appearance, themeEditor |
| Project scripts | script |
| Other | everything else (workspace, navigation, threadPanel, view, usage) |

  Rows sorted by command id then key. Empty: `No keybindings match your search.` (14px muted,
  centered, padding 48px 16px).

### 11.2 Binding row

Standard row (1.6). Title = command label + 8px + source badge (`Custom` or `Project`, outline
badge 16px high, radius 4px, 10px text; none for Default); hovering the title shows the raw
command id. Description: the When control, or for unassigned rows `No keyboard shortcut
assigned. Click Unassigned to set one.` Right side (gap 6px): conflict icon, row menu (visible on
row hover/focus), key control (margin-right -6px). Row ~72px.

When control: 24px line, gap 6px: `When` (12px, `text-muted-foreground` 70%) + micro button (20px
high, radius 4px, padding-x 5px, 11px mono, ghost when set / ghost-muted when empty) showing the
expression or `Always`, plus a 14px `ChevronDown` at 60%. Opens the builder popover.

Key control:
- Pill (shown when not recording and not dirty): 28px high, radius 6px, transparent 1px border,
  padding-x 6px; hover `border` 70% + `bg-accent`; content `KbdGroup` (gap 4px) or `Unassigned`
  (14px muted). Click starts recording.
- Otherwise an input `sm`, mono, 176px: placeholder `Press shortcut` while recording, else
  `Unassigned`. Dirty rows show a primary sm `Save` to the left.
- Kbd parts in order mod, meta, ctrl, alt, shift, key. macOS: `⌘` `⌃` `⌥` `⇧`; other: `Ctrl`,
  `⌃`, `Alt`, `⇧`; key single char uppercased, else raw token (`enter`, `f6`, `esc`).

`Kbd` (`components/ui/kbd.tsx`): 20px high, min-width 20px, padding-x 4px, gap 4px, radius 4px,
12px `font-medium` sans, `text-muted-foreground`, `bg-foreground` 7%, 12px svg.

Recording (KS:811-837): focus starts, blur stops. Tab passes through when not recording. Any Esc
stops propagation (never closes settings). Bare Esc reverts and stops. Modifier-only presses keep
recording. Accepted keys: any single character, `space`, `esc`, arrows, `f1`-`f99`, `enter`,
`tab`, `backspace`, `delete`, `home`, `end`, `pageup`, `pagedown`. macOS Meta -> `mod`, Ctrl ->
`ctrl`; elsewhere Ctrl -> `mod`, Meta -> `meta`; then `alt`, `shift`. Use the physical key when
Alt is held; Shift+digit records `shift+1`, not `!`. One chord only, no sequences. Setting a key
on an unassigned row restores the default `when`.

Conflicts (KSL:219-246): same resolved chord (`mod` -> meta on mac / ctrl elsewhere) and `when`
can overlap (either empty or the strings are equal; `"false"` never overlaps). 20x20 warning icon
(`text-warning`, 14px `TriangleAlert`), tooltip `Conflicts with {command}.` / `Conflicts with
{a, b, c}.` / `Conflicts with {a, b, c}, and more.` + ` The most recent matching binding wins when
both conditions can apply.`

Row menu (ghost-muted `icon-sm` `Ellipsis`, aria `Actions for {command}`): `Reset to default`
(Custom with a default), `Unassign` (destructive, Default rows: writes the same key with
`when: "false"`), `Remove` (destructive, non-default). No reset-all. Toasts `Unable to save
keybinding` / `The keybinding was not saved.`, `Unable to remove keybinding` / `The keybinding was
not removed.`

When builder popover (KS:600-726): width `min(544px, 100vw-32px)`, gap 12px. Header `When` +
outline compact `+ Condition` / `+ Group`. Expression input (compact, mono, placeholder `Always`;
invalid -> `destructive` border + 11px error `Use variables with !, &&, ||, and parentheses.`).
Tree editor: condition rows (`Not` toggle, variable select, remove), groups with `and` / `or`
select 96px, nested with 16px indent and 1px connector lines. Variables: `terminalFocus,
terminalOpen, previewOpen, isWeb, isDesktop, true, false, composerFocus, draftThreadRoute,
editableFocus, isMac, modelPickerOpen, previewFocus, rightPanelOpen, turnRunning,
usagePageOpen`. Unknown identifier warning `Unknown condition: {x}`. Tern v1 may ship only the
text input + validation and add the tree later.

Search matches command id, label, key string, when, or source (`default`/`custom`/`project`).

### 11.3 keybindings.json

Path `<stateDir>/keybindings.json` (section 16). JSON array, lenient read (strip `//`, `/* */`,
trailing commas), written pretty (2 spaces) + trailing newline, atomically.

```json
[
  { "key": "mod+b", "command": "sidebar.toggle" },
  { "key": "mod+d", "command": "diff.toggle", "when": "!terminalFocus" }
]
```

- Rule: `key` 1-64 chars, `command` = static command or `script.<id>.run` (`<id>`
  `^[a-z0-9][a-z0-9-]*$`, max 24), `when` optional 1-256 chars. Max 256 rules, when depth 64.
- Key grammar: lowercase, `+`-separated; `cmd`/`meta`, `ctrl`/`control`, `shift`, `alt`/`option`,
  `mod`; exactly one key token; `mod++` means the plus key; `space`, `esc`.
- When grammar: identifiers `[A-Za-z_][A-Za-z0-9_.-]*`, `!`, `&&`, `||`, parens; precedence or <
  and < not; unknown identifiers are false.
- Merge: empty file = defaults. Otherwise defaults whose command is absent from the file + all file
  rules; any file rule for a command hides all of that command's defaults. Later rules win.
- First run writes the full default list. On start, defaults for commands absent from the file are
  prepended unless their key+when is taken.
- Upsert: drop entries equal (command, normalized key, normalized when) to the new rule or to
  `replace`; when this is the command's first custom rule, also write its other visible defaults
  so they survive; append the rule. Error `Not enough room to save this shortcut and its inherited
  shortcuts.`
- Watch the file (100ms debounce) and reload. Invalid entries are ignored and reported:
  `Unable to parse keybindings config at {path}: {detail}`.
- Source label: `Project` for `script.*`, `Default` if exactly a default rule, else `Custom`.

Display label (`apps/web/src/keybindings.ts:241-276`): macOS concatenates `⌃⌥⇧⌘` + key
(`⇧⌘K`); others `Ctrl+Alt+Shift+Key`. Key labels: Space, Esc, Up/Down/Left/Right, PageUp,
PageDown, single char uppercase, else capitalized.

### 11.4 Default bindings (`packages/shared/src/keybindings.ts:58-156`)

`mod` = Cmd on macOS, Ctrl elsewhere. Labels from KSL:453-559.

| Command | Key | When | Label |
|---|---|---|---|
| sidebar.toggle | mod+b | | Sidebar: Toggle |
| workspace.focusNext | f6, mod+f6 | | Focus Next Workspace Area |
| workspace.focusPrevious | shift+f6, mod+shift+f6 | | Focus Previous Workspace Area |
| navigation.back | mod+[ | !terminalFocus | Navigation: Back |
| navigation.forward | mod+] | !terminalFocus | Navigation: Forward |
| terminal.toggle | mod+j | | Terminal: Toggle |
| rightPanel.toggle | mod+alt+b | | Right Panel: Toggle |
| threadPanel.toggle | mod+shift+b | !terminalFocus | Thread Panel: Toggle |
| rightPanel.new | mod+t | rightPanelOpen && !terminalFocus | Right Panel: New |
| terminal.split | mod+d | terminalFocus | Terminal: Split |
| terminal.splitVertical | mod+shift+d | terminalFocus | Terminal: Split Vertical |
| terminal.new | mod+n | terminalFocus | Terminal: New |
| terminal.close | mod+w | terminalFocus | Terminal: Close |
| terminal.previous | ctrl+alt+[ / cmd+shift+[ | terminalFocus && !isMac / && isMac | Terminal: Previous |
| terminal.next | ctrl+alt+] / cmd+shift+] | terminalFocus && !isMac / && isMac | Terminal: Next |
| rightPanel.close | mod+w | !terminalFocus | Right Panel: Close |
| view.reopenClosed | mod+shift+t | | Reopen Closed Tab |
| diff.toggle | mod+d | !terminalFocus | Diff: Toggle |
| preview.* (toggle mod+shift+j, refresh mod+r, focusUrl mod+l, zoomIn mod+= / mod++, zoomOut mod+-, resetZoom mod+0, find mod+f) | | previewFocus (toggle: none) | Preview: ... (keep only if Tern ships the preview panel) |
| commandPalette.toggle | mod+k | !terminalFocus | Command Palette: Toggle |
| filePicker.toggle | mod+p | !terminalFocus | File Picker: Toggle |
| projectSearch.toggle | mod+shift+f | !terminalFocus | Project Search: Toggle |
| theme.select | mod+alt+a | !terminalFocus | Theme: Select |
| appearance.cycle | mod+alt+shift+a | !terminalFocus | Appearance: Cycle |
| themeEditor.toggle | mod+alt+shift+t | | Theme Editor: Toggle |
| composer.stash | mod+s | !terminalFocus | Composer: Stash |
| thread.steerQueuedMessage | mod+shift+enter | !terminalFocus | Queue: Send First Queued Message |
| thread.editQueuedMessage | alt+arrowup | composerFocus | Queue: Edit Last Queued Message |
| composer.sendAlternate | mod+enter | composerFocus && turnRunning | Composer: Opposite Queue or Steer Action |
| composer.sendBackground | mod+enter, mod+alt+enter | composerFocus && draftThreadRoute | Composer: Start in Background |
| composer.sendAndNewThread | mod+alt+enter | composerFocus && !draftThreadRoute | Composer: Send and Start New Thread |
| chat.new | mod+n, mod+shift+o | !terminalFocus | Chat: New |
| chat.newLocal | mod+shift+n | !terminalFocus | Chat: New Local |
| chat.newWithoutProject | mod+alt+n | !terminalFocus | Chat: New Without Project |
| chat.find | mod+f | !terminalFocus && !previewFocus | Chat: Find |
| modelPicker.toggle | mod+shift+m | !terminalFocus | Model Picker: Toggle |
| composer.host | mod+shift+h | !terminalFocus | Composer: Host |
| composer.effort | mod+shift+e | !terminalFocus | Composer: Effort |
| composer.mode | mod+shift+a | !terminalFocus | Composer: Mode |
| composer.workspace | mod+shift+x | !terminalFocus | Composer: Workspace |
| composer.branch | mod+shift+g | !terminalFocus | Composer: Branch |
| composer.previousWorktree | mod+shift+l | !terminalFocus | Composer: Previous Worktree |
| pullRequest.copyNumber | mod+shift+k | !terminalFocus | Pull Request: Copy Number |
| modelPicker.previousProvider | mod+shift+arrowup | modelPickerOpen | Model Picker: Previous Provider |
| modelPicker.nextProvider | mod+shift+arrowdown | modelPickerOpen | Model Picker: Next Provider |
| editor.openFavorite | mod+o | | Editor: Open Favorite |
| thread.previous | mod+shift+[ | | Thread: Previous |
| thread.next | mod+shift+] | | Thread: Next |
| thread.copyReference | mod+shift+c | !terminalFocus | Pull Request: Copy Link or Thread |
| thread.settle | mod+shift+s | !terminalFocus | Thread: Settle |
| thread.pin | mod+shift+p | !terminalFocus | Thread: Pin |
| thread.undo | mod+z | !terminalFocus && !editableFocus | Thread: Undo |
| thread.jump.1..9 | mod+1..mod+9 | !modelPickerOpen && isDesktop | Thread: Jump: {n} |
| modelPicker.jump.1..9 | mod+1..mod+9 | modelPickerOpen && isDesktop | Model Picker: Jump: {n} |
| usage.open, usage.cost (c), usage.tokens (t), usage.limits (l), usage.period.day..quarter (mod+shift+1..4) | | usagePageOpen | Usage: ... (keep if Tern ships the usage page) |

No default, picker only: `rightPanel.toggleMaximized` (Right Panel: Toggle Maximized),
`composer.cycleHost`, `composer.increaseEffort`, `composer.decreaseEffort`, `thread.stop`,
`thread.regenerateTitle`. Scripts: `Run Script: {Title Cased id}`. Dropped:
`composer.dictation` (mod+shift+d, voice).

---

## 12. Projects (`ProjectsSettings.tsx`, `ProjectSettingsPanel.tsx`, `ProjectDefaultsSettings.tsx`)

### 12.1 Picking a project

No list of its own: the page uses the scope sentence's project menu. With `All projects`
selected it shows a notice `Choose a project to manage its name, icon, checkouts and actions.`
followed by a clickable list of projects (favicon, name, muted secondary line `Archived · <path>`,
trailing chevron) (`ProjectsSettings.tsx:51-53`, `SettingsScopeNotice.tsx:88-153`). Groups come
from the sidebar grouping (`useSettingsProjectGroups.ts`), sorted by name. Empty states: `Add a
project from the sidebar to configure it here.`, `This project is no longer available.`

### 12.2 Project detail (`ProjectSettingsPanel.tsx:539-765`), section gap 24px

0. Info alert: `Can't find a setting? Keep this project picked above and hop to any other
   settings page.`
1. Project (title hidden):

| Title | Description | Control | Writes |
|---|---|---|---|
| Name | `The shared name for this project group in the sidebar and thread lists.` | input sm, 256px; commit on blur/Enter; empty reverts with toast `Project title cannot be empty` | project `title` (DB) |
| Project icon | current icon summary (`{name} · {color}`, emoji, file path, or `Automatic`) | 24px preview + outline sm `Choose icon` + `Choose file` | `projectIcon` / `faviconPath` (DB). Reset -> both null |
| Pin | `Pinned projects come first in project pickers.` | switch | `pinned` |
| Archive | `Hides the project from project pickers. Threads and files are kept.` (archived: `Hidden from project pickers. Threads and files are kept.`) | outline sm `Archive` / `Unarchive` | `archived` |

2. `New threads` (12.3, category project: Model, Workspace).
3. Actions (12.4).
4. Repositories (12.5).
5. Worktree layout (only when the repo's `t3.json` declares `worktreeLayout`; 12.6).
6. Checkouts: `Grouping rule decides how each checkout joins project groups in the sidebar.
   Changing it can move a checkout to a different project.` One row per checkout (title = path /
   environment). Select (288px when the row is >= 512px): `Use global default`=inherit (trigger
   `Default ({mode})`), `Group by repository`=repository, `Group by repository path`=
   repository_path, `Keep separate`=separate; writes client `sidebarProjectGroupingOverrides
   [projectKey]`. Outline sm `Remove` when more than one checkout. Toast `Could not save grouping
   rule`.
7. Danger: `Remove project` / `Deletes the project entry and its threads. Files on disk are not
   touched.` / destructive-outline sm `Remove project` (Trash icon), native destructive confirm.
   Multi-checkout: `Remove this project everywhere` / `Remove all entries`. Toast
   `Failed to remove "{title}"`.

Toasts: `Failed to rename project`, `Failed to update project icon`, `Failed to pin project`,
`Failed to unpin project`, `Failed to archive project`, `Failed to unarchive project`.

Icon dialog (`ProjectIconPickerDialog.tsx`, 512px): `Choose project icon` / `Choose an icon,
emoji, or monogram.`; segmented `Icons` / `Emoji` / `Monogram`; color swatches 24px; icon search
`Search all Lucide icons`, grid 10 columns, max-height 256px, empty `No icons found.`; monogram
`Letters` (`One or two letters or numbers.`); emoji grid + `Or paste any emoji`; `Cancel` /
`Save icon`. Stores `{kind:"lucide",name,color}` / `{kind:"monogram",text,color}` /
`{kind:"emoji",emoji}`. File picker (`ProjectFaviconPickerDialog.tsx`): palette of image files,
placeholder `Search image files…`, empty `No image files found.`

### 12.3 Project defaults (`ProjectDefaultsSettings.tsx:52-631`)

Section title `New threads` (id `project-defaults`); for category `source-control` the title is
`Repositories`. Rows per category: general (Threads page) = Model, Fallback model (not in project
scope), Permissions, Workspace; project (Projects page) = Model, Workspace; source-control =
Automatically pull, Remove agent credits when merging, Default merge method.

| Title | Description (env scope / project scope) | Control | Options | Default | Key (P = project-overridable) |
|---|---|---|---|---|---|
| Model | `Default model for new threads. Projects can override it.` (automatic: `Default model for new threads, selected automatically. Projects can override it.`) / `Model for new threads in this project.` | model picker + traits picker | `No providers available` when none | null (automatic) | `defaultModelSelection` P |
| Fallback model | `Used for new threads when the default model's provider is out of usage.` | model picker + traits; trigger `None` when null | | null | `fallbackModelSelection` |
| Permissions | `Default permissions for new threads. Projects can override them.` / `Permissions for new threads in this project.` | select sm, items 256px min with icon + label + description | `Supervised`=approval-required (`Ask before commands and file changes.`), `Auto-accept edits`=auto-accept-edits (`Auto-approve edits, ask before other actions.`), `Auto`=auto (`Supported providers approve routine actions; others still ask.`), `Full access`=full-access (`Allow commands and edits without prompts.`) | full-access | `defaultRuntimeMode` P |
| Workspace | `Where new threads start. Projects and their t3.json can override it.` / `Where new threads in this project start.` | select sm | `Current checkout`=local, `New worktree`=worktree | null -> t3.json -> local | `defaultThreadEnvMode` P |
| Automatically pull | `Keeps the default branch current when the checkout has no local changes or commits. Projects can override it.` / `Keeps this project's default branch current when the checkout has no local changes or commits.` | switch | | false | `defaultAutoPull` P |
| Remove agent credits when merging | `Remove recognized agent credit lines from merge and squash messages on hosts that support it, keeping human co-authors. Includes auto-merge. Excludes merge queues, stack merges, and existing commits.` | switch | | false | `removeAgentCreditsOnMerge` P |
| Default merge method | `Pull requests start with this method. Last selected reuses whatever you chose most recently on this device.` / `Pull requests in this project start with this method.` | select sm | `Last selected`=null, `Merge`=merge, `Squash and merge`=squash, `Rebase and merge`=rebase | null | `pullRequestMergeMethod` P |

Toasts: `Default model not saved`, `Fallback model not saved`.

### 12.4 Actions (`ProjectActionsSettings.tsx`, `ProjectActionsList.tsx`)

Header row: `Actions` / `Commands that run in this project's checkout or its worktree, with
optional shortcuts.`; controls ghost xs `Import scripts` menu (only when the repo's `t3.json` has
scripts not yet added; group label `Import from t3.json`, help `Add actions declared by this
checkout without editing them first.`) and outline xs `+ Add action`. List rows: icon + name +
badges (`setup` if runOnWorktreeCreate, `on settle` if runOnSettle, `preview · desktop only` if
previewUrl), description = command (mono, truncate), control = shortcut label + hover gear
`Edit {name}`. Empty `No actions configured.` Invalid t3.json row: `t3.json is invalid` / `A
t3.json exists in this checkout but fails to parse, so every action and icon it declares is
ignored. Check the JSON syntax and icon values.`

Editor dialog (`projectScriptEditor.tsx`): `Add Action` / `Edit Action`, `Actions are
project-scoped commands you can run from the top bar or keybindings.`; Name (`Test`) + icon,
Keybinding (`Press shortcut`; `Press a shortcut. Use Backspace to clear. Shortcuts are
environment-wide. Projects using the same action share its shortcut.`), Command (`bun test`),
Preview URL (drop with preview); checkboxes `Run automatically on worktree creation`, `Wait for it
to finish before the agent starts`, `Run in the thread's worktree when the thread settles`, `Stop
this action when the thread settles`; buttons Delete / Cancel / `Save action`. Delete confirm
`Delete action "{name}"?` / `This action cannot be undone.`

Storage: scripts in `projectSettingsOverrides[projectId].defaultProjectScripts` (environment
scope: `defaultProjectScripts`); shortcut is a keybinding rule `script.<id>.run` in
keybindings.json.

### 12.5 Repositories (`ProjectRepositoriesSettings.tsx`)

Per checkout: header row title = environment label (Tern: `This computer` or the folder name),
description = workspace root, outline xs `+ Add folder`. Empty: `Repositories inside this folder
are part of the project as soon as they are there. Add a folder to bring in one from somewhere
else.` Folder rows: name + badge `Git repository` / `Folder` / `Missing` (warning), path mono,
ghost xs `Remove`. Suggested rows (from a workspace file): badge `In workspace file`, ghost xs
`Add`. Add dialog: `Track a repository or folder anywhere on this project's machine.`, `Folder
path` (placeholder `/path/to/repository`), directory browser (min-height 160px, `..` row, up to
100 entries), `Cancel` / `Add folder`. Tern can use a native folder picker instead (open
question). Stored on the project (DB): tracked folders.

### 12.6 Worktree layout (`ProjectWorktreeLayoutSettings.tsx`)

Intro `This project's t3.json sets up worktree threads with its own commands. They run on this
environment's machine with your permissions, and only as approved here: a command whose text
changes needs approval again.` Rows `Create command` (`Runs when a worktree thread starts, and when
its folder is restored`) and `Remove command` (`Runs when a thread's worktree is removed`), badge
`Approved` / `Needs approval`, xs `Approve` (outline) / `Revoke` (ghost), command in a `pre`
(max-height 192px). Approvals keyed by command text hash (DB).

---

## 13. Worktrees (`WorktreesSettings.tsx`, `BranchNamingSettings.tsx`, `StorageSettings.tsx:54-114,598-872`)

Scope sentence shown.

| Section | Title | Description | Control | Default | Key |
|---|---|---|---|---|---|
| Creation | Worktree location (hidden in project scope) | `Folder where new worktrees are created, on any drive, such as D:\worktrees or ~/worktrees. Existing worktrees stay where they are. Leave empty to use the T3 home folder.` | text input, placeholder `Default`, commit on blur, pasted paths normalized | `""` = `<base>/worktrees` | `worktreesDirectory` |
| | Start from remote | `Uses the latest commit of the base branch’s tracked remote branch, even if its name differs. Otherwise uses the same-named branch on origin. Uses your local branch if that remote branch is missing.` | switch | true | `newWorktreesStartFromOrigin` |
| | Advanced > Submodules | `How new worktrees populate git submodules. Projects and their t3.json can override it.` / project: `How new worktrees in this project populate git submodules.` | select sm: `Recursive`=recursive, `Top level only`=top-level, `Skip`=none | null -> t3.json -> recursive | `worktreeSubmodules` P |
| Branches | Worktree branch naming | `Choose how new worktree branches are named from your first message.` (semantic status: `The model chooses a prefix that describes the work, such as feat/add-search, fix/login-timeout, or refactor/auth.`) | select sm: `Static prefix`=static, `Semantic prefix`=semantic, `Custom instructions`=custom | static | `branchNamingMode` P |
| | Branch prefix (static only) | `For example, t3 or t3/ produces t3/add-search. Leave empty for no prefix.` | input, placeholder `No prefix` | `"t3"` (Tern: keep `t3` for compatibility or use `tern`, open question) | `branchNamePrefix` P |
| | Branch naming instructions (custom only) | `Appended to the naming prompt. The model returns the complete branch name; no prefix or suffix is added.` | textarea 4 rows, 672px, placeholder `Use julius/ followed by the issue ID and a short description.` | `""` | `branchNameInstructions` P |
| Cleanup | Automatic worktree cleanup (project scope only) | off `Keep this project's worktrees until you delete them manually.` / custom `Use these rules for this project.` / inherit `Use each machine's worktree cleanup settings.` | select sm `Inherit` / `Off` / `Custom` | inherit | `worktreeCleanup` P |
| | Delete worktrees with deleted threads | `Remove unused worktrees when active or archived threads are deleted. Worktrees with local changes are kept.` | switch | false | `storageCleanup.worktreeOnDelete` |
| | Delete merged worktrees | `Remove worktrees whose pull request is merged and whose commits are included in the default branch.` | switch | false | `storageCleanup.worktreeOnMerge` |
| | Delete inactive worktrees | `Remove worktrees after their threads have been inactive for this many days. Branches and thread history are kept.` | retention control (6) | null | `storageCleanup.worktreeAfterDays` |
| | Advanced > Delete settled worktrees | `Remove worktrees this many days after their thread settles. Set 0 days to remove them at the next cleanup. Branches and thread history are kept.` | retention control, min 0 | null | `storageCleanup.worktreeSettledAfterDays` |
| | Advanced > Delete unchanged worktrees | `Remove worktrees with no commits beyond the default branch.` | switch | false | `storageCleanup.worktreeUnchanged` |
| | Advanced > Keep worktrees with local changes | any: `Includes ignored files, except node_modules.` / tracked: `Deletes untracked and ignored files, including .env.` / uncommitted: `Keeps edits and untracked files. Deletes ignored files, including .env.` | select sm `Any local files`=any-local-files, `Uncommitted changes`=uncommitted-changes, `Edited tracked files`=tracked-changes | uncommitted-changes | `storageCleanup.worktreeKeepWhen` |

Say "the Tern home folder" in the location description. In project scope, cleanup rules write
`projectSettingsOverrides[id].worktreeCleanup = {mode:"custom", rules:{...all six...}}`.

---

## 14. Source control (`SourceControlSettings.tsx:461-665`, `SourceControlAccountSettings.tsx`, `SourceControlTokenSettings.tsx`, `SourceControlWritingSettings.tsx`)

Scope sentence shown. Tern keeps git + GitHub via `gh`. Sections in order:

1. `Repositories` = project defaults, category source-control (12.3).
2. `Version Control` with a rescan icon xs button (tooltip `Rescan Git and hosting
   integrations`); one row for Git: status dot over the icon (`success` available, `warning`
   missing), `Git` + version mono, summary `Available` or `Not available on this server:
   {installHint}`, read-only switch mirroring availability. Rescan also runs when the window
   regains focus.
3. `Source control providers`: one row for GitHub: dot + icon, `GitHub` + `gh` version, summary
   `Authenticated as {account}` (masked until clicked; `Click to reveal account` / `Click to hide
   account`) or `GitHub is not authenticated on this server. Sign in or configure credentials
   using the `gh` tool on the server host to enable change request features.` (Tern: drop "on
   this server" / "on the server host"); badge `Not authenticated`; switch tooltip `Offer when
   adding or publishing a project` (writes `offeredSourceControlProviders.github`, default on);
   chevron `Toggle GitHub details` expands:
   - Token form: `A token saved here is used before `GH_TOKEN` and the `gh` login, so GitHub works
     without the GitHub CLI. Give it read and write access to pull requests and contents.` +
     link `Create a token` (`https://{host}/settings/personal-access-tokens/new`). Grid
     `192px | 1fr`: `Host` (placeholder/default `github.com`), `Token` (password). Status
     `No token saved; the server uses GH_TOKEN or the gh login.` / `Saved for {hosts}.` Buttons
     outline xs `Remove`, xs `Save`. Stored in `sourceControlHosts.github.tokens[host]` (secret;
     Tern: keychain).
   - Account chooser: `Choose which `gh` login each GitHub host uses, or turn a host off.` Per
     host: name bold + `Signed in as {login}`; switch `Use GitHub on {host}`; with several logins a
     256px `Account` select (`Active gh account ({login})` then each login). Empty: `Sign in with
     `gh auth login` on the server host, then rescan to choose accounts here.` Stale pin warning
     `The chosen login is no longer signed in, so the active gh login is used.` + `Use active
     login`. Env override `{VAR} is set on the server, so it overrides the account chosen here
     until it is unset.` Writes `sourceControlHosts.github.hosts[host] = {account?, enabled}`;
     enabled + no account deletes the entry.
4. `Commits & pull requests` (id `source-control-text-generation`):

| Title | Description | Control | Default | Key |
|---|---|---|---|---|
| Commit and pull request writing | repo: `Commit messages and pull request titles and descriptions match each project's history.` / conventional: `Conventional Commit prefixes, with short pull request titles and descriptions.` / custom: `Your instructions shape commit messages and pull request titles and descriptions.` | select 224px `Repository conventions`=repo_conventions, `Conventional Commits`=conventional_commits, `Custom instructions`=custom; custom adds a 4-row textarea (672px, placeholder `Keep commit subjects under 60 characters. Keep pull request descriptions short and plain.`) | repo_conventions | `sourceControlWritingStyle.mode` / `.customInstructions` |
| Review guides (beta) | `Add a Guide tab to pull requests. An agent explains the changes and points out decisions that need review.` | switch | false | `reviewGuidesEnabled` |
| Review guide model (guides on) | `Must be a coding agent: it reads the pull request over many steps. Off uses the writer model, or Codex or Claude if that model is a text generator.` | model + traits picker + switch `Use a separate review guide model` | null | `reviewGuideModelSelection` |
| Review guide instructions (guides on) | `Your preferences for review guides: what to show first, dwell on, or skip. The guide's format and coverage stay the same.` | textarea 4 rows, placeholder `Show service interfaces before implementations. Skip test files unless they change behavior. Keep summaries to two sentences.` | `""` | `reviewGuideInstructions` |
| Advanced > Follow pull request templates | `Use the repository's pull request template when it has one.` | switch | true | `sourceControlWritingStyle.followChangeRequestTemplates` |
| Advanced > Source control writer model | `Model for commit messages, pull request text, and branch or bookmark names. When disabled, use the text generation model configured for this environment.` (drop "or bookmark") | model + traits picker + switch `Use a separate source control writer model` | null | `sourceControlWriterModelSelection` |

Empty discovery: section `Server environment`, `Nothing detected yet` / `Install Git on the
server, add optional hosting integrations or credentials your workspace needs, then rescan.` +
outline sm `Scan`. Dropped: jj row, non-GitHub hosts, GitHub App sign-in card ("Sign in with
GitHub" device flow), Issue trackers section, `SourceControlHostSettings` generic form, Pull
request sharing (Connections page).

---

## 15. Dropped pages

| Page | Why |
|---|---|
| Voice (`/settings/voice`) | voice input dropped |
| SnapShots (`/settings/snap-shot`) | snap-shot dropped |
| Browser (`/settings/browser`) | agent browser, profiles, recordings: not in the kept list (open question if Tern ships the preview panel) |
| Devices (`/settings/devices`) | devices/pairing/simulators dropped |
| Scheduled tasks (`/settings/scheduled-tasks`) | scheduled tasks dropped |
| Environments (`/settings/connections`) | remote environments, load balancing, PR sharing dropped; one process |
| Sharing & access (`/settings/access`) | network access, Tailscale, T3 Connect, guest links, device approval dropped |
| Secrets (`/settings/secrets`) | secret providers / 1Password dropped |
| Labs (`/settings/labs`) | beta flags for dropped features (guest links, linked environments, workflows, simlock, agent feedback, prompt suggestions); boring sidebar could return later |
| Diagnostics (`/settings/diagnostics`) | OTel spans, resource telemetry, failure analytics: observability dropped |
| Legacy redirects (`/settings/beta`, `/integrations`, `/storage`) | aliases only; map to Labs, Browser, Maintenance |

---

## 16. Persistence

### 16.1 Files (`apps/server/src/config.ts:150-180`, `os-jank.ts:252-258`)

T3: base = `--base-dir` > `T3CODE_HOME` > `~/.t3`; state = `<base>/userdata`.

| T3 path | Tern | Content |
|---|---|---|
| `<state>/settings.json` | `<state>/settings.json` | ServerSettings, sparse (16.2) |
| `<state>/client-settings.json` (desktop app) | `<state>/client-settings.json` | full ClientSettings object, compact JSON + `\n`; reader also accepts `{"settings": {...}}` (`apps/desktop/src/settings/DesktopClientSettings.ts:16-28`) |
| `<state>/keybindings.json` | same | KeybindingRule[] (11.3) |
| `<state>/desktop-settings.json` | same (window keys only) | `mainWindowBounds {x,y,width>=400,height>=360}` or null (fallback 1100x780), `mainWindowMaximized` |
| localStorage `t3code:theme*`, `t3code:themes:v1`, `t3code:typography-advanced` | `<state>/ui-state.json` | 9.5 |
| `<state>/themes/` | same | theme files |
| `<state>/secrets/<name>.bin` | OS keychain | secret values; settings.json holds `"••••••"` (six U+2022) |
| `<state>/statev2.sqlite` | Tern's own SQLite (spec/07) | projects (title, icon, pinned, archived, tracked folders), threads |
| `<base>/worktrees/` | same | default worktree dir |

Recommendation: Tern base = `TERN_HOME` or `~/.tern`, state `<base>/userdata`, same file names
and shapes, so a user can copy `~/.t3/userdata/{settings,keybindings,client-settings}.json` in.
Do not write into `~/.t3` while T3 Code may be running (its server rewrites and watches these
files).

### 16.2 settings.json rules (`apps/server/src/serverSettings.ts`)

- Read leniently: allow `//`, `/* */`, trailing commas. Decode per key with defaults; unknown keys
  kept on write (serde `flatten` into a `Map<String, Value>`), including unknown keys inside
  `projectSettingsOverrides[*]` and `providerInstances[*].config`.
- Write sparse: drop every value equal to its default (recursive for objects; arrays,
  `backgroundActivity`, interval keys, model selections, and null-default keys compared whole).
  2-space indent, trailing newline. Atomic: temp file in a sibling temp dir, fsync, rename; keep
  the old mode, 0600 for new files; state dir 0700.
- If the file fails to decode, run on defaults, refuse writes, and tell the user (T3 forces
  `continueThreadsAfterServerUpdate:false`, `defaultRuntimeMode:"approval-required"` in this case).
- Watch the file (100ms debounce) and reload on external edits.
- Migrations on load: keep `compactIdleThreads` mirrored with `idleCompaction`
  (`settings.ts:1728-1745`); `responseStreamingMode: "token"` reads as `paragraph`;
  `confirmQuit: true/false` reads as `hold`/`direct`; legacy `providers` -> `providerInstances`
  (only if Tern must import old files).
- Durations (`automaticGitFetchInterval`, `providerHealthRefreshInterval`) are milliseconds.

### 16.3 Keys Tern uses

Client settings (device-local, `packages/contracts/src/settings.ts:416-620`): every key in
sections 2, 3, 4, 9 plus `favorites`, `providerModelPreferences` (legacy copies),
`sidebarProjectGroupingOverrides`, `sidebarThreadSortOrder` (`updated_at`), `threadDetailsItems`,
`onboardingCompletedAt`. Ignore and preserve voice, snapshot, browser, loadBalancing keys.

Server settings (`settings.ts:1393-1644`) used by sections 5, 6, 10, 12, 13, 14:

```
providerInstances, favorites, providerModelPreferences, providerHealthRefreshInterval,
idleCompaction, compactIdleThreads, enableProviderUpdateChecks,
continueThreadsAfterServerUpdate, keepAwakeWhileAgentsWork, backgroundActivity,
backgroundActivityProfile, automaticGitFetchInterval, storageCleanup, worktreeCleanup,
worktreesDirectory, newWorktreesStartFromOrigin, worktreeSubmodules, branchNamingMode,
branchNamePrefix, branchNameInstructions, defaultModelSelection, fallbackModelSelection,
defaultRuntimeMode, defaultThreadEnvMode, defaultAutoPull, defaultProjectScripts,
removeAgentCreditsOnMerge, pullRequestMergeMethod, projectSettingsOverrides,
addProjectBaseDirectory, ghqCloneLayout, textGenerationModelSelection, threadTitleInstructions,
responseStreamingMode, snoozePresets, snoozeLimitedThreads, autoResumeLimitedThreads,
sidebarAutoSettleAfterDays, sidebarAutoSettleOnMerge, sidebarAutoSettleKeepPullRequests,
sourceControlWritingStyle, sourceControlWriterModelSelection, reviewGuidesEnabled,
reviewGuideModelSelection, reviewGuideInstructions, sourceControlHosts,
offeredSourceControlProviders
```

`ModelSelection = {instanceId, model, options?: [{id, value}]}` (decode also accepts legacy
`provider`). `projectSettingsOverrides[projectId]` holds any of the 27
`PROJECT_SCOPED_SERVER_SETTING_KEYS` (`settings.ts:1194-1263`) plus
`providerInstances: {id: {enabled?}}`; a null in a patch removes that override (except the 5
nullable keys at `settings.ts:1280-1290`, where null is a real value); an empty entry is removed.

Resolution for a project-scoped key (`packages/shared/src/projectSettings.ts:55-180`):
project override -> settings.json value -> repo `t3.json` (only `defaultThreadEnvMode`,
`worktreeSubmodules`, and only when the settings value is null) -> built-in default. A model
override naming a disabled instance is skipped.

### 16.4 Updating

UI writes go through one `update_settings(patch)` on the engine: deep-merge objects; replace
arrays, `providerInstances[id]`, `projectSettingsOverrides[id]` and `worktreeCleanup` whole;
route keys to client vs server storage by which schema owns them (`apps/web/src/hooks/
useSettings.ts:271-290`). Apply optimistically, persist, emit a change event; on failure revert
and toast. A device-only page in project scope never writes (rows are inert, 1.6).

---

## 17. Open questions

1. Share `~/.t3/userdata` with an installed T3 Code, or use `~/.tern` with import? (Recommended:
   separate dir, one-time import.)
2. Scope sentence: Tern has one environment. Keep only the project menu, and reword
   `Environment-wide setting. Select an environment to change it.` to point at All projects?
3. Browser / HTML preview panel: in or out? It decides Browser settings, `htmlPreviewWidth`,
   `preview.*` keybindings, browser artifact cleanup, and action Preview URL.
4. Usage page: keep `usage.*` keybindings and Thread details `Usage limits` item?
5. Text generators (OpenAI-compatible / Anthropic endpoints for titles) on Providers: keep?
6. `Update all` provider CLI updates and the version advisory "Update now": run the update
   command from Tern, or only show the copyable command?
7. Background activity: Tern has no remote clients. Collapse the profile select to the two
   intervals + pause switches, or keep the T3 UI verbatim?
8. Sensitive provider env vars and GitHub tokens: OS keychain (keyring crate) or a 0600 file like
   T3's `secrets/*.bin`?
9. Branch prefix default `t3`: keep for parity or change to `tern`?
10. Rename visible "T3 Code" strings (Open pull requests in T3 Code, privacy/license text, stock
    theme name) to Tern, per the trademark note in 00-plan.
11. Nav label "Source control & issues": rename to "Source control" now that issue trackers are
    dropped?
12. Environment identification (Dev/Nightly artwork): drop, or keep for Tern debug builds?
