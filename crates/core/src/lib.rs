//! Tern domain types and the app/engine contract. No IO, no GPUI.

pub mod api;
pub mod model;
pub mod provider;

pub use api::*;
pub use model::*;
pub use provider::*;

pub fn new_id() -> String {
    uuid::Uuid::now_v7().to_string()
}

pub fn now() -> Ts {
    chrono::Utc::now()
}
