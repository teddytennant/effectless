use gpui::{AnyElement, Context, Window};

use crate::views::settings::SettingsView;

#[derive(Default)]
pub struct ProvidersState {
    pub selected: Option<String>,
}

pub fn render(
    _view: &mut SettingsView,
    _window: &mut Window,
    _cx: &mut Context<SettingsView>,
) -> Vec<AnyElement> {
    Vec::new()
}
