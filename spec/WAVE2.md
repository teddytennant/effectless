# Wave 2 rules (UI view agents)

- Repo layout and APIs: read spec/00-plan.md, then crates/app/src (theme.rs, ui/, shell.rs,
  app_state.rs, store.rs, views/mod.rs). Use the existing primitives (Button, Icon, Menu,
  Popover, Tooltip, Dialogs, Toasts, Select, Switch, ToggleGroup, TextField/TextareaField)
  and theme tokens (`cx.tokens()`, `cx.states()`, `cx.tern_theme()`, `TextSize`). Never
  hard-code a color that has a token.
- Data comes from `Store` (snapshot, thread detail, send Command). The engine runs for real
  with `--demo` (EngineHandle::demo) and has rich sample threads. Core types live in
  crates/core/src. Git/terminal/providers crates are real and callable from background tasks
  (`cx.background_spawn`).
- Ground truth: spec/ref/*.png (real T3 Code 1440x900, light and dark) and
  spec/ref/computed-styles-*.json. Iterate until your screenshots match the references
  side by side. "Tern" replaces "T3 Code". Linux titlebar is 40px (refs show the web 52px).
- Build/screenshot inside your worktree:
    export CARGO_TARGET_DIR=/home/nixos/all-my-repos/frontend/tern-targets/<you>
    export CARGO_BUILD_JOBS=3
    nix develop /home/nixos/all-my-repos/frontend/tern --command cargo build -p tern
    nix develop /home/nixos/all-my-repos/frontend/tern --command scripts/shot.sh /tmp/x.png 8 -- --demo --theme dark --route thread:<id>
  (flags: --route index|draft:<id>|thread:<id>|settings/<page>|gallery,
   --layout right-panel,maximized,terminal,palette,collapsed,no-details)
  Prefer `cargo check -p tern` while iterating; RAM is tight, one cargo command at a time.
  Plain cargo, not cargo blaze.
- Ownership: edit only the files named in your brief. Small additive edits to shared files
  (views/mod.rs, shell.rs, app_state.rs, lib.rs, store.rs, ui/*) are allowed when you truly
  need them: keep them minimal and mention each one in your report. Additive core changes
  (new fields with serde defaults) likewise. Never reformat files you don't own.
- Tests for any non-trivial logic (row derivation, filtering, parsing, layout math).
  Before finishing: `cargo test -p tern`, `cargo clippy -p tern --all-targets -- -D warnings`,
  `cargo fmt -p tern` (format only your files if fmt touches others: revert those).
- Commit on your branch (w2-<you>) with short lowercase subjects of 1-4 words. No
  Co-Authored-By, no mention of AI. Don't push, don't touch main.
- Final reply: branch, what matches the refs (with the screenshot paths you compared), gaps,
  and every shared-file/core edit.
