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
//! - `views`: the content views other work fills in.

pub mod app_state;
pub mod assets;
pub mod fixtures;
pub mod gallery;
pub mod shell;
pub mod store;
pub mod theme;
pub mod tokens;
pub mod ui;
pub mod views;

