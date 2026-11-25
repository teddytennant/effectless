//! The views other work fills in. Each lives in its own file with a `pub struct
//! XView` (an entity) and a `Render` impl; the shell creates one of each and places
//! it. Read app data from [`crate::store::Store`], navigation and layout from
//! [`crate::app_state::AppState`], colors from `cx.tokens()`.
//!
//! | view | placed in |
//! |---|---|
//! | `SidebarView` | the sidebar container (width, rail, collapse are the shell's); already wrapped in `Scope::Sidebar` |
//! | `ChatView` | the chat column for Index / Draft / Thread routes: header (40px) and timeline |
//! | `ComposerView` | under the chat view in the chat column (thread routes) |
//! | `DraftView` | over the chat area for Index / Draft routes (centered composer) |
//! | `TerminalDrawerView` | under the composer when the drawer is open; the shell draws the resize handle |
//! | `DiffPanelView` | the right panel column when it is open; the shell draws the resize handle |
//! | `SettingsView` | inside the settings modal surface (nav + content) |
//! | `CommandPaletteView` | inside the palette dialog surface when the palette is open |
//! | `details::DetailsCard` | over the chat column, below the header, via `details::details_card` |

pub mod chat;
pub mod command_palette;
pub mod composer;
pub mod details;
pub mod diff_panel;
pub mod draft;
pub mod settings;
pub mod sidebar;
pub mod terminal;
pub mod terminal_drawer;

pub use chat::ChatView;
pub use command_palette::CommandPaletteView;
pub use composer::ComposerView;
pub use diff_panel::DiffPanelView;
pub use draft::DraftView;
pub use settings::SettingsView;
pub use sidebar::SidebarView;
pub use terminal_drawer::TerminalDrawerView;
