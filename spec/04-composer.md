# 04 · Composer

Source of truth: `forks/t3code/apps/web/src` (paths below are relative to it unless they start
with `packages/` or `apps/server/`). Units: 1 Tailwind unit = 4px, 1rem = 16px (root font size
follows the Interface font size setting, default 16px). `sm:` = window width ≥ 640px; Tern is a
desktop app, so **use the `sm:` value** and treat the `max-sm` value as the narrow-window fallback.
Colors are CSS tokens from `index.css` (spec 01 owns the palette). `/N` after a token means that
alpha, e.g. `border/70` = `--border` at 70%.

Radius scale (`index.css:295-300`, `--radius` = 8px at `:1148`): `sm` 4, `md` 6, `lg` 8, `xl` 12,
`2xl` 16. `--control-radius` = 8px (`:103`). Text: `3xs` 10px, `2xs` 11px, `xs` 12px, `sm` 14px,
`base` 16px.

Dropped for Tern (do not build): voice dictation (`ComposerSpeechButton`, recording pill, live
transcription box, `composer.dictation`), guest names/attribution, MCP app elicitation UI, cloud
(load balancing, peer machines), telemetry, the collapsed mobile composer
(`showCollapsedMobilePromptRow`, `showInlineRestingControls`), SnapShot animations.

---

## 1. Layout

### 1.1 Vertical stack

Cited: `components/ChatView.tsx:3820-4086`, `components/chat/ChatComposer.tsx:3631-4195`.

```
┌ QueuedRunsControl (timeline bottom, §8.1) ───────────────────────┐
│   banner dock: attached notices + pending-question drawer + Stash│  inset 22px each side,
│   tab (§7, §8.2). Tucks 17px under the card.                     │  slides under card top
├──────────────────────── composer card ───────────────────────────┤
│ body  px16 pt16 pb8                                              │
│   [queued-edit attachment row] [media strip] [file list]         │
│   editor (2 lines min, 208px max)                                │
│ validation line (only when set)                                  │
│ footer px8 pb8                                                   │
│   [📎]                     [model ▾ effort][perm][…]  [⏹/➤]      │
└──────────────────────────────────────────────────────────────────┘
  context strip (env · worktree · branch · new-thread Permissions)  ← spec for branch toolbar
```

### 1.2 Card surface

`components/chat/ComposerSurface.tsx:5-85`, tokens `index.css:119-131,151-152,200-201,2378-2386`.

| Layer | Property | Value |
|---|---|---|
| Form / Shell | width | 100% of chat column, centered |
| | max width | `--chat-content-max-width`: 736px (Chat width "comfortable", default), 1152px ("wide"), 100% ("full") |
| Shell `::before` (glass) | radius | 16px |
| | fill | `--chat-composer-glass-surface` at `--glass-opacity` 80%. Light: `--card`. Dark: `--surface-raised`. Custom theme: `--app-theme-surface-raised` |
| | backdrop | blur 12px, saturate 1.14 (dark: blur 16px, saturate 1.08). No backdrop support: opaque fill |
| Host | radius | 16px |
| | shadow | `--shadow-composer` = `0 1px 2px rgb(0 0 0 / 5%)` light; none in dark |
| Host `::after` (outline) | border | 1px `--chat-composer-outline`: light `rgb(0 0 0 / 8%)`, dark `white 5%`, custom theme `--app-theme-toolbar-border` |
| | dark inset highlight | `inset-shadow-2xs` in `--chat-composer-highlight` = `rgb(255 255 255 / 3%)` |
| Main | padding | 1px all sides; radius 16px |
| Inner surface div | radius 16px; `transition: background-color 200ms` |
| Drag over (files/thread) | | bg `--accent`/45 + 1px inset ring `--primary`/70 |
| No project picked yet | | opacity .75 |
| Ultrathink active | | 2px rainbow ring (`index.css:2450-2491`): `linear-gradient(120deg,#ff6b6b 0%,#f59e0b 18%,#22c55e 36%,#14b8a6 54%,#3b82f6 72%,#ec4899 90%,#ff6b6b 100%)`, saturate .82 brightness .92, plus `inset 0 0 0 1px rgba(255,255,255,.07)` (`components/chat/composerProviderState.tsx:129-134`) |

`--chat-composer-drawer-inset` = 1.375rem (22px). Everything attached above the card (banners,
slash menu, stash) is `card width − 44px` wide and offset 22px from the card's left edge.

### 1.3 Body and editor

`ChatComposer.tsx:3833-4030`, `components/ComposerPromptEditorTiptap.tsx:1146-1170,2011-2070`.

| Item | Value |
|---|---|
| Body padding | left/right 16px, top 16px (14px narrow), bottom 8px |
| Editor font | `--font-composer` (falls back to `--font-sans`), size `--font-size-prompt` = 14px (Appearance setting may override in px) |
| Line height | 1.625 (`leading-relaxed`) → 22.75px at 14px |
| Min height | 2 lines + 8px (box is `-4px` margin + `4px` padding, so it lays out as exactly 2 lines, 45.5px) |
| Max height | 208px, then scrolls vertically |
| Wrapping | `white-space: pre-wrap`, break long words |
| Text color | `--foreground`; placeholder `--placeholder` (= `--muted-foreground`), same line height, overlays line 1 |
| While a send is held (images still compressing) | editor block opacity .65, 150ms fade |

Placeholder, first match wins (`ChatComposer.tsx:3959-3977`, `composerPlaceholder.ts:3`):

| State | Placeholder |
|---|---|
| Approval pending, provider takes feedback | `Send feedback to decline this request` |
| Approval pending, no feedback support | `Send a message to decline and redirect the agent` |
| Question pending, choice-only | `Choose an option above` |
| Question pending, no options | `Type your answer` |
| Question pending, with options | `Type your own answer, or leave this blank to use the selected option` |
| Draft with no project chosen | `Choose a project above to start a thread` |
| No provider enabled | `Enable a provider in Settings to send a message` |
| Prompt suggestion available (optional, §6) | the suggestion text + outline Kbd `⇥ Tab` (ml 8px) |
| Default | `Ask anything. @ files, $ skills, / commands` |

Editor aria-label: `Message {title}` on a thread, `Message` on a new draft. Editor is
read-only when: no project chosen, choice-only question, answer being submitted.

### 1.4 Attachment rows (inside body, above the editor)

See §5. Media strip and file list each have 12px bottom margin.

### 1.5 Validation line

`components/chat/ComposerPromptLengthValidation.tsx`. Between body and footer: `<p role=alert>`
px 16px, pb 8px, 12px text, `--destructive`. Text is the provider input error or submission
error, e.g. over 120,000 chars.

### 1.6 Footer row

`ChatComposer.tsx:4050-4167`, `components/chat/ComposerControlsBar.tsx:148-314`,
`components/chat/composerControlsOwnership.ts:6-31`.

Row: `display:flex; align-items:center; flex-wrap:nowrap`, padding 0 8px 8px (+8px top when a
question is pending). Left to right:

| # | Control | Size | Icon / content | Shown when |
|---|---|---|---|---|
| 1 | Attach | 28×28 ghost icon button (32 narrow), radius 8, `margin-right:8px` | lucide `Paperclip` 16px, nudged 1px down. Tooltip `Attach files` | server accepts files and no question forbids attachments (`ChatComposer.tsx:1281-1285`) |
| 2 | spacer + right group | `flex:1; justify-content:flex-end; gap:8px` | | always |
| 2a | Controls cluster | `flex:1; justify-content:flex-end; gap:2px` | 3, 4, 5 | always |
| 3 | Model + effort trigger | ComposerControl `sm` (below) | provider icon 16px, model short name, effort suffix | always (or the provider-unavailable button) |
| 4 | Permissions chip | ComposerControl `sm`, `padding-left:6px` | runtime-mode icon 16px + label | only when the thread's mode differs from the user's default mode and the context strip is not already naming it |
| 5 | Overflow `…` | ComposerControl `sm` | lucide `Ellipsis` 16px, aria `More composer controls` | only when width measurement pushed item 4 out; its menu holds the Permissions section |
| 6 | Compact toggle | h 28px, px 8px, radius 8 | `Minimize2` 16px muted + `Compact`/`Keep` + token count muted, tabular nums. Label hidden under 480px card width | only when a resume would summarize history (`resumeCompactionTokens`) |
| 7 | Send / Stop | 32×32 circle | §1.7 | always |

When no provider is usable, items 3-5 collapse into one ComposerControl: `CircleAlert` 16px +
`Open provider settings` (or `No provider available`, disabled).

**ComposerControl `sm`** (`components/chat/ComposerControl.tsx:325-334`): height 28px, padding
0 8px, gap 6px, radius 8, 1px transparent border, font-weight 500, 14px, color
`--foreground`/80, icons 16px in `--muted-foreground` unless they set a color. Hover: bg
`--accent`, text `--foreground`. Pressed / popup open: bg `--accent`. Active (not menu)
scale .97. Disabled opacity .64. Focus ring 2px inset `--ring`.

### 1.7 Send / Stop button states

`components/chat/ComposerPrimaryActions.tsx:101-551`.

Send: 32×32 circle (36 narrow), bg `--message-action` (= `--primary`), fg
`--message-action-foreground`, `shadow-xs` in `--message-action`/24, inset top highlight
`0 1px rgb(255 255 255/16%)`; hover bg `--message-action-hover` (primary 90% over background)
and scale 1.05; pressed inset `0 1px rgb(0 0 0/8%)`, no shadow. Disabled: bg `--foreground`/12,
fg `--muted-foreground`, no shadow, no hover scale. Default glyph is a custom SVG 14×14:
`M7 11.5V2.5M7 2.5L3 6.5M7 2.5L11 6.5`, stroke 1.8, round caps/joins.

| State | Condition | Glyph (16px unless noted) | aria-label / tooltip |
|---|---|---|---|
| Empty | no sendable content | arrow, disabled | `Submit message` |
| Ready | idle, has text or attachment | arrow | `Submit message` |
| Busy | send in flight | `Spinner` sm | `Submitting message` |
| Held | waiting for images to compress | `Spinner` | `Sending when ready` |
| Preparing worktree | | arrow, disabled | `Preparing worktree` |
| Env down | | disabled | `Environment disconnected` |
| Steer | turn running, follow-up = steer (default), no ⌘/Ctrl held | `CornerUpRight` | label `Steer message`; tooltip `Click to steer, ⌘-click or {shortcut} to queue` (`Ctrl` off macOS) |
| Queue | turn running and (follow-up = queue, or ⌘/Ctrl held while follow-up = steer, or turn can't take steering) | `ListPlus` | `Queue message`; tooltip `Click to queue, ⌘-click or {shortcut} to steer` |
| Editing queued msg | | `Check` | `Update queued message` / busy `Updating queued message` |
| Resume | thread resumable, composer empty | `Play` filled; button type=button | `Resume thread` |
| Compact first | resume needs compaction and Keep is off | arrow | `Compact and send`; tooltip `Summarize {tokens} tokens of history, then send` |

The Queue/Steer glyph tracks live modifier state (⌘ on macOS, Ctrl elsewhere) while hovering.

**Stop** replaces Send when a turn can be interrupted and the composer holds nothing sendable:
32×32 circle, bg `--destructive`/90 (hover `--destructive`), white, `shadow-xs`
`--destructive`/24, inset highlight. Glyph: 12×12 SVG, `<rect x=2 y=2 w=8 h=8 rx=1.5>` filled.
Tooltip `Interrupt`; after click: aria `Stopping`, tooltip `Stopping…`, disabled until the run
stops or 10s pass (then re-enabled). For 500ms after a stop lands, a click on whatever replaced
it is swallowed. If the user types while running, Send (Steer/Queue) shows and, while a stop is
pending, Stop stays to its right.

**Pending question** (§7.2) swaps the Send circle for a pill: h 28px (32 narrow), px 16px
(12 compact), radius full, same message-action colors, 14px weight 500. Labels: `Next question`,
`Submit answer` (first question), `Submit answers`, `Submitting...`; compact: `Next`/`Submit`.
When past question 1, an outline `sm` button `Previous` (compact: 28px outline icon
`ChevronLeft` 14px, aria `Previous question`) sits to its left, gap 8px (6 compact). Stop, if
interruptible, follows at 28px.

---

## 2. Model picker

### 2.1 Trigger

`components/chat/ProviderModelPicker.tsx:150-445`, `components/chat/ComposerControlsBar.tsx:41,187-250`,
`components/chat/providerIconUtils.ts:30-44`, `packages/provider-core/src/modelShortName.ts`.

ComposerControl `sm`, `min-width:52px`, shrinks; label `max-width:224px` truncated. Popup open:
bg `--accent`, text `--foreground`. Content, gap 6px:

1. Provider instance icon 16px (brand glyph per driver; a small badge 12px at bottom-right
   `-2px,-2px` when two instances share a driver). In multi-model mode: up to 3 overlapping 16px
   icons (`-4px` overlap, 2px ring in glass surface color) then `+N` 16px circle, 10px text.
2. Model name: short name. Short name strips the provider's own family prefix: `Claude `,
   `GPT-`/`GPT `, `Gemini `, `Grok `, `Muse `, but only if words other than version numbers
   remain. `Claude Opus 5.5` → `Opus 5.5`; `GPT-6 Astra` → `6 Astra`; `GPT-5.5` stays
   `GPT-5.5`. No selection: `Choose model` (`Choose models` in multi mode).
3. Effort suffix (§3.1), muted, turns `--foreground` on hover/open, `margin-left:-2px`.
4. If the model is unavailable: label `{model} (Unavailable)` + outline badge `Unavailable`.

Opens on click, `⌘⇧M`, `/model`, `⌘⇧E` (opens and focuses the effort column), `⌘⇧A` when no
permissions chip or overflow holds Permissions (opens the footer's Permissions menu). Disabled
while the provider catalog loads or a send is busy. Each open asks the server to refresh the
model catalog (`server.refreshProviders {modelCatalogOnly:true}`).

### 2.2 Popup layout (composer, "docked")

`ProviderModelPicker.tsx:446-505`, `components/chat/ModelPickerContent.tsx:957-1260`,
`components/chat/ModelPickerSidebar.tsx:50-234`, `components/chat/ModelListRow.tsx`,
`components/chat/ComposerModelEffort.tsx:538-742`, `components/ui/popover.tsx:93-110`, `index.css:481-495`.

Opens **above** the trigger (`side: top`), aligned to the trigger's end edge, shifts to stay on
screen. Popup: radius 8, `dropdown-glass` (bg = `--popover` 18% over `--popover` at 80% alpha,
blur 12/16px, 1px border `--contrast-foreground`/10), shadow `0 16px 40px -18px rgb(0 0 0/55%)`
(dark `0 18px 44px -18px rgb(0 0 0/80%)`), top inner hairline `0 1px black/4%` (dark
`0 -1px white/6%`). Enter/exit: scale .98 + opacity 0.

```
┌────┬──────────────────────────────┬────────────────────┐ ─┐
│ ★  │ 🔍 Search models...          │ Reasoning  Opus 5.5│  │
│────│──────────────────────────────│────────────────────│  │
│ ◎▌ │ ◎ Opus 5.5            ✓  ☆ ⌘1│ Low                │  │ 346px
│ ◎  │ ◎ Sonnet 5.5             ☆ ⌘2│ Medium     ✓       │  │
│ ◎  │ ◎ Fable 5.2   NEW        ☆ ⌘3│ High               │  │
│    │ Show more                    │ ...                │  │
├────┴──────────────────────────────┴────────────────────┤ ─┘
│ [Standard|⚡Fast] [⚙ Ultracode]          🔒 Full access ⌃│  footer
└────────────────────────────────────────────────────────┘
 44px        ≤ 316px                       192px
```

| Region | Spec |
|---|---|
| Body row | height 346px fixed (shrinks to available height; footer always kept) |
| Sidebar | width 44px, bg `--muted`/30, padding 4px, gap 4px, vertical, scrollbar hidden |
| Sidebar item | square (36×36), radius 6, hover/focus bg `--foreground`/10. Favorites: `Star` 20px filled. Then a 1px `--border`/70 divider. Providers: instance icon 20px in a 24px box. Unavailable/locked: opacity .5, not-allowed |
| Selected marker | 3×20px pill, `--primary`, radius full, 4px from the sidebar's right edge, vertically centered on the selected item, animates `top` 200ms ease-out |
| Sidebar tooltip | left side, 8px offset: provider display name; `Favorites`; unavailable: `{label}: Disabled in settings.` / `{label}: Unavailable. {message}` / `{label}: Limited. {message}` / `{label}: Not ready. {message}`; locked: `{provider} is unavailable in this thread. Start a new thread to switch providers.` |
| List column | max width 360px including sidebar, `flex:1`, bg `--muted`/40, 1px left border `--border`/70 |
| Search | label padding 12px 12px 0 (top 10px); underline 1px `--border`/70 (focus `--ring`), 6px below; `Search` icon 16px `--muted-foreground`/55 at left, top 6px; input h 26px, text indent 20px, 14px. Placeholder `Search models...` |
| List | vertical padding 6px, left padding 8px, virtualized, 2px gap between rows, top/bottom scroll fades |
| Row | min-h 28px (32 narrow), radius 4, padding 4px 8px, gap 8px; hover/highlight bg `--foreground`/9 |
| Row content | instance icon 16px · name 14px regular, truncate (title tooltip only if clipped) · `Check` 14px if selected · sub-provider 12px `--muted-foreground`/70 · `NEW` badge (10px bold uppercase, tracking wide, radius 4, px 2, border `--update`/35, bg `--update`/15, text `--update-foreground`) · outline badge `Unavailable` |
| Row trailing | gap 6px: star button 24px ghost-muted, `Star` 12px; filled `--warning` when favorite, else visible only on row hover/highlight. Tooltip `Add to favorites` / `Remove from favorites`. Then jump Kbd (`⌘1`…`⌘9`) for the first 9 enabled visible rows |
| Disabled row | tooltip on left with the reason |
| Legacy toggle row | last row: `Show more` / `Show less`, 14px `--muted-foreground` (hover `--foreground`) |
| Sub-provider heading (fx only) | name 14px + `{count} models` 12px muted below, `ChevronRight` 16px rotating 90° when open; unavailable: outline badge `Unavailable` + reason, not expandable |
| Empty | `No favorites yet` (Favorites tab), `Searching project models...`, `No models found` |
| Setup notices | under list, 1px top border, max-h 176px scroll, 12px muted text (3 lines max) + inline button `Open provider setup` / `Set up {provider}`; update-required line + inline button `Provider settings` |
| Effort column | width 192px, 1px left border `--border`/70, bg `--muted`/40 |
| Effort header | padding 10px 12px 0; inner h 33px, 1px bottom border `--border`/70, 6px bottom padding, 14px: descriptor label (`Reasoning`, or `Options`/`Effort`) muted, right-aligned model short name 12px muted truncated |
| Effort rows | container padding 10px 8px 6px, gap 2px, scrolls. Row h 32px, px 8px, radius 4, 14px `--foreground`; hover/focus bg `--foreground`/9; selected: `Check` 14px at right; default (not selected): `Default` 12px `--muted-foreground`/70 at right. Option description → tooltip on the right, max-w 224px |
| Other options (non-reasoning) | below a 1px divider (mt 4, pt 4). Select: two-line row (12px muted label over 14px value) + `ChevronRight` 14px; opens a side menu to the right (6px offset, max-w 288px) of radio items with description (12px, 2 lines) and `Default`. Boolean: same two-line row + small Switch |
| No levels | one 32px row `No effort levels`, 14px muted |
| Ultrathink typed in prompt body | note above the levels (12px muted/80): `Your prompt contains "ultrathink" in the text. Remove it to change this option.` Levels disabled |
| Footer | flex-wrap, gap 8px, 1px top border `--border`/70, bg `--muted`/40, padding 8px 12px |
| Speed segmented control | container radius 8, bg `--input`/40, padding 2px. Segment h 24px, px 8px, 12px muted (hover `--foreground`), checked bg `--input`/72 text `--foreground`. Codex: one segment per service tier (`Standard`, `Fast`, `Ultrafast`…) with tier description as tooltip. Claude: `Standard` / `⚡ Fast` from the `fastMode` boolean |
| Fast bolt | lucide `Zap` 12px filled. Ultrafast: two bolts, the back one outlined at x+3px, cut by a 4px stroke in the glass color |
| Ultracode toggle | same segment style, single toggle, `Workflow` icon 12px + label |
| Permissions menu button | pushed right (`margin-left:auto`), h 28px, px 8px, radius 6, 12px muted; current mode icon 14px + label + `ChevronUp` 12px; hover/open bg `--foreground`/9. Opens §3.3 section above it, end-aligned |

Non-composer pickers (settings etc.) use the same list without the effort column: max height
346px, width 360px, closes on pick. Not needed for this spec.

### 2.3 Behavior and keyboard

`ModelPickerContent.tsx:440-560,700-955,1052-1145`, `ModelPickerSidebar.tsx:89-104`,
`packages/client-runtime/src/modelSearch.ts`, `apps/web/src/modelOrdering.ts`.

- **Initial tab**: thread's provider if locked, or if its model needs setup/is unavailable;
  else Favorites when any favorite is pickable; else the active provider.
- **Order in a provider tab**: favorites first, then server catalog order. Legacy models
  (manifest `status: legacy`) that are not favorites go behind the `Show more` row, which
  starts expanded if the selected model is legacy. Favorites tab: favorites in provider order.
- **Search**: typing hides the sidebar and searches every provider (only the locked one in an
  existing thread). Multi-token: each token must match one of name, short name, sub-provider,
  driver id, provider display name, slug. Per field, score = exact < prefix < word boundary <
  substring < fuzzy (fuzzy only for tokens ≥3 chars), earlier fields rank better. Exact full
  name or short name wins outright. Favorites get a boost. Ties keep catalog order. The popup
  freezes its height when search starts.
- **Keys** (window capture listener while open, so they beat the composer):

| Key | Action |
|---|---|
| typing | filter (focus starts in search) |
| ↑ / ↓ | move highlight (auto-highlights first row) |
| Enter | pick highlighted; on legacy/sub-provider rows, toggle them |
| Shift+Enter / Shift+click | add/remove from multi-model selection (only if multi-model is enabled) |
| ← (empty search) or Shift+Tab | focus sidebar's selected provider |
| → in sidebar | back to search |
| → at end of search text | pick the highlighted model if different, then focus its effort levels |
| ↑ ↓ / letter in effort column | move between rows / type-ahead by label |
| ← in effort column | back to search |
| ⌘⇧↑ / ⌘⇧↓ | previous / next provider tab (cycles, includes Favorites, skips disabled), clears search |
| ⌘1…⌘9 | pick the Nth enabled visible model |
| Esc | close |

- **Docked behavior**: picking a model does **not** close the popup; focus moves to that model's
  selected effort level. Picking an effort level also leaves it open. Escape or outside click
  closes. Picking calls `onProviderModelSelect(instanceId, slug)`; that writes the draft's (or
  thread's) model selection.
- **Locked provider**: an existing thread can only switch models within its provider (and its
  continuation group); other providers are disabled with the tooltip above.

### 2.4 Providers and models

Providers, in driver registration order (`apps/server/src/provider/builtInDrivers.ts:33-45`,
display names `packages/contracts/src/model.ts:197-209`, defaults `:144-158`):

| Driver id | Display name | Default model | Catalog source |
|---|---|---|---|
| `codex` | Codex | `gpt-6-astra` | `codex app-server model/list` at runtime |
| `claudeAgent` | Claude | `claude-opus-5-5` | `apps/server/src/provider/model-manifest.json` + probe |
| `cursor` | Cursor | `auto` | agent at runtime |
| `grok` | Grok | `grok-build` | agent at runtime |
| `fx` | fx | `default` | agent, with sub-provider sections |
| `opencode` | OpenCode | `openai/gpt-5` | agent at runtime |
| `antigravity` | Antigravity | `antigravity-default` (hidden, resolves to the agent's default) | manifest |
| `kiro` | Kiro | `default` | agent `model` option |
| `pi` | Pi | `default` | user's Pi settings |
| `muse` | Muse Code | `default` | agent |
| `acpRegistry` | ACP Registry | `default` | registry agent |

Only enabled, visible instances appear in the sidebar. Model lists are server data
(`ServerProvider.models`: `slug, name, shortName?, subProvider?, badge?: "new", isLegacy?,
isUnavailable?, isDefault?, capabilities.optionDescriptors`); Tern must render whatever the
server sends. Manifest snapshot (`model-manifest.json`, updated 2026-11-20) for reference:

| Provider | Current (shown) | Legacy (behind `Show more`) |
|---|---|---|
| Codex | `gpt-6-astra` GPT-6 Astra, `gpt-6.1-sol` GPT-6.1 Sol, `gpt-6-luna` GPT-6 Luna, `gpt-daybreak-blue-latest`, `gpt-daybreak-red-latest` | GPT-6 Sol, GPT-5.6 Sol, GPT-5.6 Terra, GPT-5.6 Luna, GPT-5.5, GPT-5.4 Mini, GPT-5.3 Codex Spark |
| Claude | Claude Opus 5.5, Claude Sonnet 5.5, Claude Fable 5.2, Claude Haiku 5.5 | Fable 5.1, Fable 5, Opus 5, Opus 4.8, Opus 4.7, Opus 4.6, Opus 4.5, Sonnet 5, Sonnet 4.6, Haiku 4.5 |
| Antigravity | Gemini 4 Carbon, Gemini 4 Argon, Gemini 3.8 Flash, Gemini 3.8 Flash (High) (default), (Medium), (Low) | Gemini 3.7 / 3.6 Flash (High/Medium/Low), Gemini 3.1 Pro (High) (`gemini-pro-agent`), Gemini 3.1 Pro (Low) |

Favorites are per environment (`useEnvironmentModelPreferences`), keyed `instanceId:slug`.

---

## 3. Effort, permissions, interaction mode

### 3.1 Effort suffix on the trigger

`ComposerModelEffort.tsx:86-185`, `components/chat/EffortGlyph.tsx`.

The reasoning control is the model's `select` descriptor with id `effort`, `reasoningEffort`,
`variant`, `thinking`, or `role: "reasoning"`. Setting **Appearance → Effort display**
(`label` default, or `bars`):

| Case | Suffix |
|---|---|
| label mode | current level's label, 14px, max-w 96px: `Low`, `Medium`, `High`, `Extra High` … |
| bars mode | bar glyph: one 2px bar per ordinary level, 4px pitch, heights rise from ~half to 12px in whole-pixel steps inside a 14px box (12px box when `xs`), sitting 2px above the box bottom; bars up to the current level opacity 1, rest .4. A leading `off`/`none` level is not drawn. Only for 2-7 ordinary levels |
| named levels (`max`, `ultracode`, `default`, `ultra`, prompt-injected like `ultrathink`) | always the word |
| boolean reasoning (`thinking`) | `{label} On` / `{label} Off` |
| + Fast / Ultrafast speed | bolt after the word |
| + Ultracode toggle on | `Workflow` 12px |
| + context window option | its label, 12px, max-w 96px (`200k`, `1M`) |

Trigger tooltip/aria-label: `{model}, {detail}` where detail lists the chosen traits.

### 3.2 Effort values per provider

Server-defined; per-model option descriptors. Codex (`apps/server/src/provider/CodexProvider.ts:171-345`):

| Descriptor id | Label | Values (label) |
|---|---|---|
| `reasoningEffort` | Reasoning | from `supportedReasoningEfforts`: `none` None, `minimal` Minimal, `low` Low, `medium` Medium, `high` High, `xhigh` Extra High, `max` Max, `ultra` Ultra. Default = model's default (GPT-6 Astra forced to `medium`) |
| `serviceTier` | Service Tier | `default` Standard, then server tiers (`fast` Fast, `ultrafast` Ultrafast "Even faster, more expensive") → footer speed control |
| `cyberAccessProgram` | Cyber Access | `standard` Standard, `daybreakBlue` Daybreak Blue, `daybreakRed` Daybreak Red → "other options" row |

Claude (`model-manifest.json` profiles):

| Model | `effort` "Reasoning" levels (default *) | Other |
|---|---|---|
| Fable 5.2, Opus 5.5 | Low, Medium*, High, Extra High, Max, Ultracode, Ultrathink | `fastMode` "Fast Mode" (bool) |
| Fable 5 | same | none |
| Opus 5, Opus 4.8 | Low, Medium, High*, Extra High, Max, Ultracode, Ultrathink | Fast Mode |
| Opus 4.7 | Low, Medium, High, Extra High*, Max, Ultrathink | Fast Mode |
| Opus 4.6 | Low, Medium, High*, Max, Ultrathink | Fast Mode; `contextWindow` 200k / 1M* |
| Opus 4.5 | Low, Medium, High*, Max | Fast Mode |
| Sonnet 5.5 | Low, Medium, High*, Extra High, Max, Ultracode, Ultrathink | |
| Sonnet 5 | Low, Medium, High*, Extra High, Max, Ultrathink | |
| Sonnet 4.6 | Low, Medium, High*, Max, Ultrathink | `contextWindow` 200k* / 1M |
| Haiku 5.5 | Low, Medium*, High, Extra High, Max, Ultracode, Ultrathink | |
| Haiku 4.5 | none | `thinking` "Thinking" bool (on) |

Ultracode description: `xhigh effort plus multi-agent workflow orchestration`. Ultrathink is
prompt-injected: picking it prepends `Ultrathink:\n` to the prompt and turns on the rainbow
frame (§1.2); removing the prefix reverts. If the word "ultrathink" appears in the prompt body
the levels lock (§2.2).

**What is sent**: `modelSelection = { instanceId, model, options: [{id, value}] }` on
`thread.turn.start` (`useComposerSend.ts:1577-1600`, `packages/contracts/src/modelSelection.ts`),
e.g. `[{id:"effort",value:"high"},{id:"fastMode",value:true}]`. Only explicit picks are sent.

Shortcuts `composer.increaseEffort` / `composer.decreaseEffort` step the level (unbound by
default; `components/chat/composerShortcuts.ts:15-20`).

### 3.3 Runtime mode (Permissions)

`components/chat/runtimeModeConfig.ts:13-126`, `components/chat/ComposerFooterControls.tsx:21-192`,
`packages/contracts/src/providerPolicy.ts:30-47`.

| Value (sent as `runtimeMode`) | Label | Description | lucide icon |
|---|---|---|---|
| `approval-required` | Supervised | Ask before commands and file changes. | `Lock` |
| `auto-accept-edits` | Auto-accept edits | Auto-approve edits, ask before other actions. | `PenLine` |
| `auto` | Auto | Supported providers approve routine actions; others still ask. | `ShieldCheck` |
| `full-access` | Full access | Allow commands and edits without prompts. | `ShieldOff` |

Default `full-access`. Order above is fixed; a provider reporting `supportedRuntimeModes`
narrows the list, and an unsupported current mode falls back to the first supported.
An org policy ceiling disables higher modes with reason `{organization} limits threads to {mode}.`

**Permissions section** (used by the footer chip menu, the overflow menu, the picker footer
menu, and the new-thread context strip): width 288px, padding 6px 8px, gap 6px.
1. Heading `Permissions`, 12px weight 500 muted.
2. Segmented radio row: radius 8, bg `--input`/40, padding 2px; one segment per mode, h 24px,
   `flex:1`, icon 14px muted; checked bg `--input`/72 + `--foreground`; outer segments get
   left/right radius 6; disabled opacity .4. ←/→ move between segments.
3. Preview: mode label 14px (`--foreground` when it is the current mode, muted while previewing
   another), description 12px/16px muted, reserved 2 lines (min-h 32px). Hover or focus on a
   segment previews it. Click picks and closes.

Footer chip (§1.6 #4): tooltip `Permissions: {mode}. {description}`, aria `Permissions: {mode}`,
icon in label color when the mode differs from default. New-thread strip version is
ComposerControl `xs` with a 12px icon and tooltip `Permissions · {shortcut}` (owned by the
branch-toolbar spec). `⌘⇧A` opens whichever control currently holds Permissions.

### 3.4 Interaction mode (chat vs plan)

Not in this fork's UI. `ProviderInteractionMode = "default" | "plan"` survives in contracts and
the draft store, but every composer send hard-codes `interactionMode: "default"`
(`useComposerSend.ts:1595`), there is no plan toggle or `/plan` command, and the composer
filters the `agent: plan` option out of provider options (`TraitsPicker.tsx:133-138`). Tern:
send `"default"`, build no toggle.

---

## 4. Triggers, slash commands, mentions

### 4.1 Trigger detection

`composer-logic.ts:262-297`, `packages/shared/src/composerTrigger.ts:71-104`.

The token is the run of non-whitespace before the caret.

| Prefix | Kind | Rule |
|---|---|---|
| `/` | slash-command | query = rest. Not if the word contains a second `/` or `\` (a path) |
| `#` | pull-request | at token start or after an opening bracket, followed by letters/digits/`_-` |
| any currency symbol (`$`, `€`, `£`…) | skill | not if the rest looks like an amount (`$5`) |
| `@` | path (files, folders, threads, machines, models) | |

Escape dismisses the trigger for that token; typing a new trigger reopens.

### 4.2 Slash menu contents

`components/chat/useComposerMenuItems.ts:256-333`, `composerSlashCommandSearch.ts`,
`ChatComposer.tsx:2066-2250`.

Built-in, in this order:

| Label | Description | Selecting it | Availability |
|---|---|---|---|
| `/model` | Switch response model for this thread | removes the `/model` token, opens the model picker | always |
| `/side` | Ask a side question in a new side chat | inserts `/side ` | side chat available; only at prompt start |
| `/btw` | Same as /side | inserts `/btw ` | same |
| `/clear` | Start a fresh session; earlier messages stay visible | removes token, arms fresh session (banner `Your next message starts a fresh session` / `The agent won't see earlier messages in this thread.`, dismiss `Keep the current session`) | thread exists; only at prompt start |

Then the provider's own slash commands (`ServerProvider.slashCommands`, label `/{name}`,
description from the provider or its input hint, fallback `Run provider command`); selecting
inserts `/{name} `. `/usage-limits` ("Show this provider's usage limits") opens the usage
dialog instead. Provider `/compact` is hidden unless compaction is allowed; provider `/clear`
is hidden when T3's own `/clear` is offered. Codex adds `/goal` and `/feedback`
(`CodexProvider.ts:1165-1170`). Provider commands work only at prompt start.

Then skills (setting "Show skills in slash menu", on by default): label `/skill:{name}`,
rendered as muted `/skill:` + display name; selecting inserts `${name} `. Description: short
description, description, or `{scope} skill`.

Ranking (query with leading `/` stripped): command name exact 0, prefix 2, boundary (`-_/`) 4,
substring 6, fuzzy 100+; description 20-26; best of the two; tiebreak built-in → provider →
skill, then name. Empty query: list order.

Commands the composer runs itself when sent as the whole message (never queued/steered):
`/side [prompt]`, `/btw [prompt]`, `/clear`, `/usage-limits`, Codex `/feedback [reason]`
(`packages/client-runtime/src/state/composerDispatch.ts:59-74`).

`$` menu: provider skills searched by name, label = skill display name, description fallback
`Run provider skill`, badge on the right showing source: `App`/`Repo`/`Project`/`Personal`/
`System`/`Provider` with `Blocks`/`Folder`/`Folder`/`UserRound`/`Settings`/`Package` icons
(in `$` menu the badge reads `App Skill` etc.). Empty: `No skills found. Try / to browse provider commands.`

### 4.3 Menu visuals

`components/chat/ComposerCommandMenuLayer.tsx`, `components/chat/ComposerCommandMenu.tsx:109-305`,
`components/chat/ComposerBanner.tsx:42-73`, `components/ui/command.tsx:190-210`.

- Portaled, `position:fixed`, z 40. Left = card left + 22px; width = card width − 44px.
  Bottom edge sits 17px **below** the card's top edge (tucked under the card). Max height =
  max(96px, space above − 24px). Mounted one frame after the trigger appears.
- Surface: attached banner surface: top corners 16px, bottom square and hidden under the card;
  same glass fill and outline as the card (§1.2), top-to-bottom mask fading the bottom 16px into
  the card; `shadow-composer` (dark `0 14px 32px -18px rgb(0 0 0/75%)`); 17px bottom padding.
- List: max height 288px, padding 8px, scrolls. Item: min-h 28px, padding 6px 8px, gap 8px,
  radius 8; highlighted (mouse move or ↑↓) bg `--accent`, text `--accent-foreground`.
- Item content: leading 16px icon (file-type icon for paths, `MessagesSquare` for threads,
  machine icon, provider icon for models, PR state icon) · label 12px weight 500, `max-width:45%`,
  truncate · description 12px `--secondary-label`, truncate, `flex:1` (tooltip with full text if
  clipped and highlighted) · optional badge.
- Empty/loading: padding 14px 20px 28px, 12px `--secondary-label`: `Searching workspace files...`,
  `Searching workspace skills...`, `Finding pull request...`, `No matching files or folders.`,
  `No matching command.`, PR texts from `useComposerMenuItems.ts:429-450`.
- Keys while open: ↑/↓ move and wrap around (`ChatComposer.tsx:2260-2271`), Enter or Tab picks the
  highlighted item (first item if none), Esc closes. Shift+Enter and ⌘Enter are not captured.

### 4.4 `@` mentions

`useComposerMenuItems.ts:211-255`, `apps/web/src/state/queries.ts:51,364-373`,
`packages/shared/src/composerTrigger.ts:45-49`.

- Search: RPC `projects.searchEntries` in the thread's cwd, limit 80, empty query allowed (shows
  top entries). Results: label = basename (NFC), description = parent directory path, icon by
  file type/folder. Preceded by matching threads, machines, and models (slop for Tern, §9).
- Picking a path replaces the token with a Markdown link `[basename](path)` plus one space
  (`./` prefix added when the path would otherwise parse oddly; label and destination escaped).
  A following space already in the text is absorbed.
- **The prompt value is plain Markdown text**; chips are only how the editor displays links.
- Chip (`ComposerPromptEditorTiptap.tsx:270-338`, `components/ContextChip.tsx`): inline atom,
  not editable, selectable as one unit (selected: `Highlight` overlay at 30%, radius 4).
  `display:inline-flex; align-items:baseline; gap:.2em`, weight 500, file icon at 1em, label
  truncates, no background. Color `--context-chip-ink` falls back to the inherited text color
  (the `--context-chip-*` accent tokens are not defined anywhere in this fork). Hover underlines
  the label. Click opens a file preview. Hover tooltip shows the path. Backspace deletes it whole.
- Skill (`$name`) and model mentions render as chips the same way.
- Drag a file from the workspace tree onto the composer inserts a mention.

`FileSearchResults.tsx` is the timeline's tool-result renderer, not part of the composer.

---

## 5. Attachments

`components/chat/useComposerAttachmentIntake.ts:140-533`, `components/chat/ComposerAttachmentStrip.tsx`,
`packages/contracts/src/chatAttachment.ts:13-17,208-226`, `packages/client-runtime/src/textPaste.ts`.

Intake: paste, drop on the composer, the paperclip (native multi-file picker, any type).

| Limit | Value |
|---|---|
| Attachments per message | 100 → `You can attach up to {max} files per message.` |
| Image types | GIF, HEIC, HEIF, JPEG, PNG, WebP (by MIME or extension) else `''{name}'' is not a supported image type. Attach GIF, HEIC, HEIF, JPEG, PNG, or WebP images.` |
| Image size | compressed client-side to ≤ 10 MiB; else `''{name}'' is too large to attach, even after compression.` |
| Total images | 80 MiB per message |
| Other files | ≤ 50 MiB or the server's staging limit, whichever is lower; empty files rejected `''{name}'' is empty or could not be read.`; server without file support `This server does not support file attachments.` |
| Prompt text | 120,000 chars |
| Large paste | text ≥ 32 KiB (chars or UTF-8 bytes), or a paste that would exceed 120,000 chars, becomes a file `pasted-text.txt` (`pasted-text-2.txt`…) with toast `Large paste attached as {name}` / `{size} · Use ⌘⇧V to keep a large paste inline.` ⌘⇧V (Ctrl+Shift+V) pastes as plain text inline |

Errors land in the thread error banner, toasts as quoted.

Where an image goes: if the prompt already has text (or the paste targets a selection, or a
question is open), the image is inserted as an inline chip at the caret; otherwise it goes to
the media strip. Files always add an inline chip reference.

**Media strip** (images, previewable videos): flex-wrap, gap 8px, 12px bottom margin.
Tile 64×64, radius 8, 1px `--border`/80, bg `--background`, image `object-fit:cover`; click
opens the image viewer (`cursor: zoom-in`). Overlays:
- Remove: top-right 4px inset, 24×24 `media-close` button (bg `black/65`, white `X`, 1px ring
  `white/20`, hover `black/80`), aria `Remove {name}`.
- Preparing/uploading: bottom bar `--background`/85, 10px text centered: `Preparing` or upload %.
- Upload failed: bottom-left 24px overlay button `RefreshCw`, tooltip with the failure.
- Not persisted: top-left 12px `CircleAlert` on `--background`/85, `--warning-foreground`,
  tooltip `Draft attachment could not be saved locally and may be lost on navigation.`
- Fallback when undecodable: `Image` icon 16px `--secondary-label`.
Videos: same 64px tile on black with `Play` 16px and a bottom gradient.

**File list**: column, gap 4px, 12px bottom margin. Row: gap 8px, py 4px, 14px: file-type icon
· name (button, truncate, underline on hover, opens preview dialog) · status 12px
`--secondary-label` (size, upload %, `Attach again`, `Remove to send`) · retry 24px ghost ·
remove 24px ghost `X`.

Removing an image that is referenced in the text asks `Remove {name} from the message?\nIt is
referenced in your text; removing it also removes every reference.`

---

## 6. Keyboard

`composer-logic.ts:57-108`, `components/chat/composerCommandKey.ts:223-305`,
`packages/shared/src/keybindings.ts:96-145`, `components/chat/composerPromptHistory.ts:197-218`.
`⌘` = Cmd on macOS, Ctrl elsewhere. Ignored during IME composition and key repeat.

Setting **Send shortcut** (`sendShortcut`): `enter` (default), `mod-enter-multiline` (⌘Enter
only once the prompt has a newline), `mod-enter` (always ⌘Enter). Setting **Follow-up
behavior** (`followUpBehavior`): `steer` (default) or `queue`.

| Key | Context | Action |
|---|---|---|
| Enter | send shortcut = enter | send (dispatch: idle → start turn; running → follow-up behavior) |
| Shift+Enter, Alt+Enter | | newline (list continuation applies) |
| ⌘Enter | turn running | send with the *other* follow-up action (steer↔queue) |
| ⌘Enter | new draft route | send in background (start the thread, stay on a fresh draft) |
| ⌘⌥Enter | new draft | send in background; existing thread: send and open a new thread |
| ⌘⇧Enter | anywhere (not terminal) | steer the first queued message into the running turn |
| ⌥↑ | composer focused | edit the last queued message |
| ↑ | caret on first visual line, prompt empty or showing a recalled entry, no attachments/contexts, no question | recall previous sent user message of this thread (skips automation/agent messages and consecutive duplicates; strips `Ultrathink:\n`, trailing review-comment and legacy context blocks) |
| ↓ | caret on last visual line while browsing | next newer entry; past the newest returns to empty |
| ↑ / ↓ | menu open | move highlight |
| Tab / Enter | menu open | pick |
| Tab | empty prompt with suggestion | accept the suggestion |
| Tab / Shift+Tab | caret in a list item | indent / outdent the item |
| Esc | send held | cancel the held send |
| Esc | menu open | dismiss the trigger |
| Esc | empty prompt with suggestion | dismiss the suggestion |
| 1-9 | question pending, focus not in a text field | pick option N |
| ⌘S | | stash the prompt / toggle stash menu (§8.2) |
| ⌘⇧M | | toggle model picker |
| ⌘⇧E | | open model picker on the effort column |
| ⌘⇧A | | open Permissions |
| ⌘⇧H / ⌘⇧X / ⌘⇧G / ⌘⇧L | | host / workspace / branch picker / previous worktree (context strip, other spec) |
| ⌘⇧↑ / ⌘⇧↓, ⌘1-9 | picker open | provider tabs / jump to model |
| PageUp / PageDown | caret at editor edge | scroll the timeline |

There is no keyboard interrupt; Stop is click-only. There is no plan-mode toggle.

---

## 7. Pending approval and pending question

### 7.1 Approvals

`components/chat/TimelinePendingApprovalRow.tsx`, `components/chat/ComposerPendingApprovalActions.tsx`,
`ChatComposer.tsx:1266-1275`.

Approvals render **in the timeline**, not over the composer. The composer only changes:
placeholder (§1.3), and a send in this state declines the visible approvals and steers the
waiting turn with the typed text (follow-up forced to steer). An approval that lands while the
user is typing is held back by a typing guard so the draft is not sent as its answer.

Timeline row (for reference, owned by the timeline spec): request kind icon, the command/path
in 12px mono (max-h 160px), reason, then `🛡 Waiting for approval` (`--warning`) · inline
buttons `Decline` (muted) `Approve` (default) and `More` menu (`Cancel`, `Always allow this
session`). Server-provided options replace these labels. Request kinds: `Command approval`,
`File change approval`, `File read approval`, `App permission approval`, `App access approval`.
Decision values: `accept`, `acceptForSession`, `decline`, `cancel`.

### 7.2 Pending question (user input request)

`components/chat/ComposerPendingUserInputPanel.tsx`, `ChatComposer.tsx:3695-3730`,
`components/chat/ComposerBanner.tsx:117-300`.

Shown as a drawer attached above the card (same attached-surface style and 22px inset as §4.3,
`variant: info`). One request at a time; multi-question requests page through questions.

| Part | Spec |
|---|---|
| Header row (button, toggles collapse) | padding 6px 16px; grid `[28px icon col][1fr][auto]`; header text 12px muted truncate; collapsed adds ` · ` + question text 12px `--secondary-label` |
| Header actions | `{n}/{total}` count when >1 question · chevron 14px (rotated when collapsed) · dismiss `X` 14px in a 24px ghost button (aria `Dismiss question without answering`) if dismissible |
| Body | max height min(384px, 40vh), scrolls; padding 0 16px 4px 22px |
| Question | Markdown, 12px/16px, `--foreground`/85 |
| Options | 6px below; button rows: gap 10px, py 6px, ps 6px, radius 6; selected `--foreground`, unselected `--foreground`/70 with hover bg `--muted`/30. Radio 16px circle (1px `--foreground`/55 border, checked fill `--primary` with 6px `--primary-foreground` dot) or checkbox for multi-select. Label 14px weight 500, description 11px `--secondary-label`. Right: Kbd `1`…`9` in a 24px column. Option preview Markdown below, 12px, indent 32px |
| Single-select | click (or digit key) selects and auto-advances after 200ms |
| Free text | the composer editor holds the custom answer (placeholders §1.3); typed text overrides the option |
| Footer | send button becomes the Next/Submit pill (§1.7) |

---

## 8. Queue, stash, drafts

| Feature | Verdict |
|---|---|
| Queue / steer while running | **core** |
| Edit queued message in composer | core-lite (do after the basic queue) |
| Draft persistence per thread | **core** |
| Prompt history (↑) | **core** |
| Stash | slop for v1 |
| Prompt suggestion (Tab) | slop |
| Multi-model send, `@model`, `@thread`, `@machine`, `#PR` mentions | slop |
| Compact-before-resume toggle | later |

### 8.1 Queued messages

`components/chat/QueuedRunsControl.tsx:255-470,520-600`, `packages/client-runtime/src/state/composerDispatch.ts:9-21`.

Dispatch mode sent as `dispatchMode` on `thread.turn.start`: idle → `auto`; running and turn
can't steer → `queue`; otherwise the follow-up setting (`steer` default), flipped by ⌘.
Optimistic message carries `inputIntent: "queued_turn"` or `"steer"`.

Rendered at the bottom of the timeline, right above the composer, centered in the chat width,
`padding-bottom:16px`: right-aligned list, gap 12px. Each item:
- Bubble: max-w 80%, radius 16, **1px dashed** border `--muted-foreground`/40 (editing:
  `--primary`/60 and `--foreground` text), text 14px `--muted-foreground`, padding 8px 12px,
  full text `pre-wrap`. Image thumbs 32×32 radius 6 above the text.
- Action row under it, right, 12px muted, gap 2px: ghost-muted `xs` button `↑ Send now`
  (tooltip `Send into the current turn ({shortcut})`, or `Start a turn with this message`,
  `Skip compaction and send this message`, `Move this command to the front of the queue`) ·
  `🕒 Saving` while pending · edit `Pencil` 24px (tooltip `Edit in the composer ({shortcut})`)
  · remove `X` 24px (tooltip `Remove from queue`). While editing: `Editing in the composer` +
  ghost `Cancel`.
- More than 3 queued: show 2, then inline muted button `Show all {count} queued` / `Show fewer`
  with chevron 14px.

Editing a queued message loads it into the composer; Send shows `Check` / `Update queued message`.

### 8.2 Stash (slop)

`components/chat/ComposerStashBadge.tsx`, `ComposerStashMenu.tsx`, `useComposerStash.ts:520-640`,
`promptStashStore.ts:15`. Global (not per thread), max 20 entries, in localStorage.
⌘S with a prompt: saves prompt + attachments + contexts as an entry and clears the composer.
⌘S on an empty composer: restores the only entry, else toggles the menu. Badge: an attached
tab at the dock's right, `Bookmark` icon + `Stash` + count, pulses on each stash. Menu: attached
drawer, header `Stash {count}`, rows `FileText` icon, snippet (truncate), image thumbs 16px,
file count, relative time, delete `X`. Empty: `Nothing stashed yet. Press {shortcut} with a
prompt in the composer to stash it.`

### 8.3 Draft persistence

`composerDraftStore.ts:119-127,336-430`.

One draft per thread key (and per unsent draft thread), persisted to localStorage key
`t3code:composer-drafts:v1`, schema version 9, writes debounced 300ms. Per draft: `prompt`,
image attachments, `files`, terminal contexts, preview annotations, review comments, thread /
environment contexts, `replyTo`, `modelSelectionByProvider`, `activeProvider`,
`modelSelectionExplicit`, `runtimeMode`, `interactionMode`. Store-level:
`stickyModelSelectionByProvider` (last picked model/options per provider, seeds new drafts),
draft-thread metadata (project, env mode local/worktree, branch, worktree path).
Tern: one JSON file in the app data dir with the same shape, 300ms debounce, cleared on send.
The composer stays mounted across threads; switching threads swaps the draft in place and resets
history browsing.

---

## 9. Rich text: what to build

`components/ComposerPromptEditorTiptap.tsx:858-975`, `composer-list-continuation.ts`,
`composer-code-block.ts`, `components/chat/ComposerCodeBlockNodeView.tsx`.

The value is always a Markdown string. The setting **Rich text composer**
(`composerRichTextEnabled`, default on) only changes how that string is displayed and edited.

| Behavior | Mode | Minimal Tern |
|---|---|---|
| Multiline plain text, pre-wrap, undo/redo with grouped steps | both | **required** |
| Atom chips for `[name](path)` file links, `$skill` | both | **required** (render inline, delete as a unit) |
| List continuation on Enter (`- `, `* `, `+ `, `1.`/`1)` incrementing with zero padding, `- [ ] ` tasks); following ordered items renumber; Enter on an empty item drops to the parent item's marker, or removes the marker at top level | both (works on the string) | **required** |
| Tab / Shift+Tab indent/outdent list items | both | **required** |
| Code fence: typing ```` ```lang ```` then Enter opens a code block; Enter on trailing blank lines or a closing fence exits; Tab indents inside | rich | recommended |
| Code block rendering: framed block with language picker header, shiki highlighting, mono, pre 12px/8px padding | rich | later |
| WYSIWYG marks `*em*` `**bold**` `~~strike~~` `` `code` `` shown styled with markers hidden | rich | drop for v1 |
| Headings, blockquote, horizontal rule, bullet/ordered/task list rendering | rich | drop for v1 |
| RTL per-paragraph direction | both | drop |
| Thread/PR/model/citation/terminal/preview-annotation chips | both | drop with their features |
| Bracket auto-pairing table (`(`, `[`, `{`, quotes, backtick, `*`, `_`, `«`) around a selection | both | optional |

Minimal faithful implementation: a GPUI multiline text input holding the Markdown string, with
inline chip runs for links/skills, the list-continuation and Tab rules from
`composer-list-continuation.ts` (pure string functions, port them directly), the trigger
detector from §4.1, and the 2-line min / 208px max sizing. This matches the app with
"Rich text composer" turned off, which is a supported user setting, so it is pixel-correct for
that configuration.

---

## Open questions

1. Tern's default for `composerRichTextEnabled`: matching T3's default (on) means building
   inline Markdown rendering and code blocks; starting with it off is faithful to the "off"
   setting only.
2. `--context-chip-*` color tokens are referenced but never defined in this fork, so chips
   render in inherited text color. Confirm that is the look to match, not a bug to fix.
3. The model trigger shows `6 Astra` for `GPT-6 Astra` because of the short-name rule. Keep
   it, or special-case Codex?
4. Which providers ship in Tern v1? The picker is data-driven, but each driver needs an icon
   and a server.
5. Queued-message list and approval rows live in the timeline. Confirm the timeline spec owns
   them; this file only fixes their composer-facing behavior.
