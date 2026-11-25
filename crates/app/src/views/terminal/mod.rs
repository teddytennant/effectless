//! The terminal drawer (spec 05 §3) and its pieces.
//!
//! | module | what |
//! |---|---|
//! | `drawer` | `TerminalDrawerView`: per-thread terminals, toolbar, tab sidebar, splits |
//! | `pane` | `TerminalPane`: one PTY surface, keys, mouse, selection, links, menu |
//! | `element` | the grid renderer (merged background quads, shaped text runs) |
//! | `model` | per-thread ids, groups and active terminal |
//! | `layout` | cell metrics, grid fit, scrollbar and wheel math |
//! | `input` | key bindings, key and mouse encoding decisions |
//! | `colors` | theme tokens and ANSI palettes |
//! | `selection` | selection ranges in scrollback coordinates |
//! | `open` | opening URLs and paths |

pub mod colors;
pub mod drawer;
pub mod element;
pub mod input;
pub mod layout;
pub mod model;
pub mod open;
pub mod pane;
pub mod selection;

pub use drawer::{TerminalDrawerEvent, TerminalDrawerView};
pub use input::bind_keys;
pub use pane::TerminalContext;
