# effectless

T3 Code's desktop app rewritten as one native Rust binary on [GPUI](https://www.gpui.rs).
No Electron, no Node server, no Effect, no telemetry.

Unofficial. Not affiliated with, endorsed by, or maintained by T3 Tools, Inc. It's an
independent rewrite derived from the MIT-licensed [T3 Code](https://github.com/pingdotgg/t3code)
source. The app inside is called Tern and uses none of T3's names, logos or icons.

It drives Codex (`codex app-server`) and Claude (the `claude` CLI, stream-json) on your
machine, keeps threads in SQLite under `~/.tern`, checkpoints every turn with hidden git refs,
and gives you diffs, worktrees, commit/push/PR through `gh`, and a real terminal.

Work in progress. The backend crates are done and tested; the UI views are being built.

## Build

Linux, with Nix:

```
nix develop --command cargo build --release -p tern
```

Without Nix you need pkg-config plus the fontconfig, freetype, xkbcommon, wayland, vulkan and
X11 dev packages.

## Run

```
target/release/tern           # real agents, state in ~/.tern
target/release/tern --demo    # sample projects and threads, touches nothing
```

`codex` and/or `claude` must be on your PATH and signed in.

## Layout

`crates/core` types, `store` SQLite, `providers` Codex and Claude drivers, `git` checkpoints,
diffs and worktrees, `terminal` PTY, `engine` orchestration, `app` the GPUI binary. `spec/`
holds the specs the UI is built against and reference screenshots of T3 Code.

## Left out on purpose

Telemetry and analytics, cloud sign-in, T3 Connect and remote access, the mobile app, voice,
scheduled tasks, workflows, sharing, the updater, and the GitHub star prompt.
