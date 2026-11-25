# Tern: plan

Tern is a native GPUI rewrite of the T3 Code desktop app (pingdotgg/t3code, MIT).
It's unofficial and not affiliated with T3 Tools, Inc. The layout and behavior
match T3 Code. The name and icon don't (see t3code TRADEMARK.md).

One process. No Node server, no websocket, no browser. The app runs the agents,
stores threads in SQLite, runs git, and hosts terminals itself.

## Dropped on purpose

Telemetry, product analytics, observability exporters, Clerk/cloud auth, T3 Connect
and the relay, remote environments, devices and pairing, guest sharing, the mobile
app, marketing site, i18n machinery (English only), voice input, snap-shot, scheduled
tasks, workflows, secret providers, issue trackers, non-GitHub source-control hosts,
the desktop updater, the GitHub star prompt, MCP app frames, HTML render frames.

## Workspace

```
crates/
  core/       domain types (serde), ids, settings + keybinding schema. no gpui, no io
  store/      SQLite persistence (rusqlite bundled): projects, threads, messages,
              activities, checkpoints, settings
  providers/  agent drivers: codex (app-server JSON-RPC over stdio), claude
              (stream-json CLI). One trait, events into core types
  git/        git + worktrees + per-turn checkpoints + diffs + gh pr
  terminal/   PTY (portable-pty) + alacritty_terminal grid
  engine/     orchestration: owns sessions, applies provider events to thread state,
              persists through store, emits change notifications. tokio runtime
  app/        the GPUI binary `tern`: theme, shell, sidebar, chat timeline, composer,
              diff panel, terminal drawer, settings, command palette
```

The UI only talks to `engine` through a handle: commands in, snapshots/events out.
UI code never spawns processes or touches SQLite.

## UI stack

gpui + gpui-component (crates.io). Its pieces are restyled to T3's tokens: Input for
the composer, scroll, popover, menu, dialog, tooltip, notification. Tokens live in
`app/src/theme.rs`, generated from spec/01.

## Specs

01 tokens + primitives, 02 shell/sidebar/palette, 03 chat timeline, 04 composer,
05 panels (diff, terminal, git), 06 settings, 07 backend + providers.
Every UI number in the code should trace back to one of these.
