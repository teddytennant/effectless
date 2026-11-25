# T3 Code reference screenshots

Ground truth for Tern, taken from the real T3 Code web UI.

- T3 Code **v0.1.12**: npm `t3@0.1.12`, whose launcher downloads the self-contained linux-x64 build from the v0.1.12 GitHub release. The fork at `forks/t3code` (f99eceb7) carries the same version string.
- The server ran as `t3 serve --port 3773 --host 127.0.0.1` with `HOME` and `T3CODE_HOME` set to temp dirs. Telemetry was off (`T3CODE_TELEMETRY_ENABLED=false`, `DO_NOT_TRACK=1`) and nothing was signed in. `CLAUDE_CONFIG_DIR` pointed at the real Claude login so the Claude provider worked.
- Headless Chrome 147 at 1440x900 with device scale 1. Light and dark come from emulating `prefers-color-scheme`. Fonts and finite animations settled before each shot, and the text caret is hidden.
- Chrome ran with `--blink-settings=primaryHoverType=2,availableHoverTypes=2,primaryPointerType=4,availablePointerTypes=4` so it reports a mouse. Without that flag headless Chrome matches neither `(hover: hover)` nor `(pointer: fine)`, which turns off every Tailwind `hover:` style and switches on the `pointer-coarse:` variants. Only the files flagged "pre-fix" below were taken without it.
- Fixture project: `demo-notes`, a throwaway git repo with `README.md`, `notes.py`, `test_notes.py`, `pyproject.toml` and `.gitignore`.
- Model: Claude Haiku 5.5, Low effort. Four prompts in total. Codex 0.160.1 shows "Not authenticated" because this box's Codex is set up for xAI, not OpenAI.

Every state has a `-light.png` and a `-dark.png` unless noted.

## Images

| File | State |
| --- | --- |
| `01-welcome` | First run: the welcome/onboarding page (agents list, project import offer). Separate fresh server. "Import 6 projects" counts this machine's Claude history, and "nixos" is the host name |
| `02-no-projects` | Main shell with no projects: empty sidebar, "What should we work on?" |
| `02b-no-projects-button-hover` | Same, with the pointer on the "Add project" button |
| `03-add-project-sources` | Add project palette: New project / Local folder / Git URL / GitHub repository |
| `04-add-project-local-folder` | Local folder browser at `/tmp/t3ref/repo/` |
| `05-draft-composer-focused` | Project added, new-thread draft screen with the composer focused and the thread details card at top right |
| `06-model-picker-open` | Model picker open: provider rail, model list, reasoning column, footer |
| `06b-model-picker-row-hover` | Model picker with the pointer on a model row (star button shows) |
| `07-command-palette` | Command palette (Ctrl+K), empty query |
| `07b-command-palette-filtered` | Command palette filtered by "theme" |
| `07c-command-palette-thread-search` | Command palette matching a thread ("list") |
| `08-settings-general` | Settings modal, General |
| `09-settings-appearance` | Settings, Appearance |
| `10-settings-chat` | Settings, Chat |
| `11-settings-providers` | Settings, Providers (Codex unauthenticated, Claude authenticated) |
| `12-settings-keybindings` | Settings, Keybindings |
| `13-settings-archive` | Settings, Archive with one archived thread (the fork) |
| `13b-settings-nav-hover` | Settings nav with the pointer on "General" (Archive pane still selected) |
| `14-draft-prompt-typed` | Draft with the first prompt typed, not sent |
| `15-thread-streaming-00-light`, `-01-dark`, `-02-light`, `-03-dark` | **Pre-fix.** Frames ~3s apart while the first turn streamed: "Working for 5s", live reasoning line, "Running head" tool row, the sidebar "Working" section with a spinner row, the red stop button, then the finished answer while the sidebar animates. One scheme per frame, alternating, because the turn only took 9s |
| `16-thread-complete` | Finished thread: user bubble, collapsed "Worked for 8.8s", markdown table, Python code block, closing paragraph |
| `17-thread-worklog-expanded` | "Worked for" expanded to "Ran 2 commands and changed 1 file" with the four work rows |
| `17b-thread-tool-row-expanded` | One command row expanded to its highlighted command and output |
| `17c-thread-assistant-hover` | Pointer on the assistant answer: copy / fork / reply actions and timestamp |
| `17d-thread-user-message-hover` | Pointer on the user message |
| `18-diff-panel` | Right panel, Diff tab, unified view of the `notes.py` change (+2 -0) |
| `18b-diff-panel-split` | Same diff in split view |
| `19-right-panel-file-viewer` | Right panel with `notes.py` open in the file viewer plus the file tree |
| `20-terminal-drawer-with-diff` | Terminal drawer open under the thread, diff panel still open. Ran `git diff --stat && ls` (twice, because the terminal survived a browser restart) |
| `21-terminal-drawer` | Terminal drawer with the right panel closed |
| `22-permission-popover` | Permission mode popover (Supervised / Auto-accept edits / Auto / Full access) |
| `23-approval-pending` | Thread in Supervised mode waiting on approval for `touch TODO.md && ls`: "Waiting for approval · Decline Approve More", composer reads "Send feedback to decline this request", sidebar row shows the "Approval" badge |
| `23b-approval-more-menu` | Approval "More" menu (Cancel, Always allow this session) |
| `23c-approval-approve-hover` | Pointer on "Approve" |
| `24-thread-forked` | **Pre-fix.** Thread forked from the first answer: "Forked from conversation" divider and the Lineage section in the details card. The fork was archived afterwards and archived threads redirect to a draft, so it couldn't be retaken |
| `25-sidebar-context-menu` | Right-click menu on a sidebar thread row |
| `25b-sidebar-context-menu-snooze-submenu` | Same menu with the Snooze submenu open |
| `26-sidebar-states` | Sidebar with three threads: pinned and finished, finished, waiting on approval |
| `27-sidebar-row-hover` | Pointer on the pinned row: hover tooltip (title, project, host, branch, model) and the hover row actions |
| `27b-sidebar-row-hover-unpinned` | Pointer on an unpinned row: snooze icon and "Settle" replace the timestamp. The tooltip is up too |

## Computed styles

- `computed-styles-thread.json`: `getComputedStyle` values from the finished thread (the `16-thread-complete` layout), `light` and `dark` keys. Covers sidebar rows (row, content box, title, meta line), the sidebar panel, composer editor, placeholder and surface, user message text and bubble, assistant paragraph and markdown root, code block container, `pre` and a token, inline code, table `th`/`td`, the "Worked for" label, titlebar, breadcrumb title and project crumb, titlebar icon button, model picker button with its label and effort spans, the submit button, and `body`/`html`. Each entry has font-family, size, weight, line-height, letter-spacing, color, background, padding, margin, size, borders, radius, shadow and the element's rect. `root_css_vars` holds every resolved `--*` custom property on `:root` (129 of them).
- `computed-styles-hover.json`: rest vs hover colors for an inactive sidebar row, the model picker button, a titlebar icon button and the submit button.

The composer surface isn't one box. The fill is a `::before` on the `group/composer-surface` element (card at 80% alpha with `backdrop-filter: blur(12px) saturate(1.14)` in light, blur 16px and saturate 1.08 in dark). The 1px outline is an `::after` on the `shadow-composer` child, and that child carries the `0 1px 2px rgb(0 0 0 / 0.05)` shadow. The JSON records all three layers.

Fonts as rendered here: UI text uses the bundled "Inter Variable". The monospace stack starts with `ui-monospace, "SF Mono"` and ends up as Liberation Mono on this Linux box, so code in these shots is Liberation Mono, not what a Mac would show.

## Not captured, or captured with caveats

- No dark/light pair for the streaming frames (see above), and the sidebar "Working" state only exists in the pre-fix `15-*` frames. A later 40s `sleep` turn was meant to provide it, but the browser had to be restarted for the pointer fix and the turn was finished by then.
- `ls` alone never asked for approval: Claude Code auto-approves read-only commands even in Supervised (`default`) mode. The approval shots use `touch TODO.md && ls` instead, which was never approved or declined.
- A fresh install defaults to Opus 5.5 Medium. After the first switch the draft screens show Haiku 5.5 Low, because the selection sticks.
- The small "=" glyph left of the second user message in `23-*` is part of the UI as rendered. It's there with and without the pointer fix.
- `25-*`, `26-*`, `27-*` and `27b-*` show a blue focus ring on the approval's "More" link. That's left over from closing the More menu with Escape, not part of the sidebar state.
- No Codex thread, because Codex isn't signed in to OpenAI here.
