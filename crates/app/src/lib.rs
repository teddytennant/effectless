//! Tern: a native GPUI client for coding agents, laid out like T3 Code.
//!
//! Module map:
//! - `theme`: the `Theme` global (tokens, typography, radii, shadows, motion,
//!   contrast, palette scopes) and gpui-component theme sync.
//! - `ui`: T3's primitives (Button, Badge, Kbd, Tooltip, Menu, Popover, Dialog,
//!   Toast, Switch, Checkbox, Toggle, Select, Input, Icon, …).
//! - `shell`: window chrome, titlebar strip, sidebar container, panel slots,
//!   overlay hosts.
//! - `app_state`: `AppState` (route, layout); `store`: `Store` (engine data).
//! - `keymap`: T3's commands and keybindings (spec 02 §5); register handlers here.
//! - `views`: the content views other work fills in.
//! - `markdown`: markdown → GPUI (chat replies, plans, descriptions).

pub mod app_state;
pub mod assets;
pub mod fixtures;
pub mod gallery;
pub mod keymap;
pub mod markdown;
pub mod shell;
pub mod store;
pub mod theme;
pub mod tokens;
pub mod ui;
pub mod views;
