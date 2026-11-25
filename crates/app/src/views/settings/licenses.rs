use gpui::{AnyElement, Context, Window};

use super::SettingsView;

#[derive(Default)]
pub struct LicensesState {
    pub open: Option<usize>,
}

pub fn render(
    _view: &mut SettingsView,
    _window: &mut Window,
    _cx: &mut Context<SettingsView>,
) -> Vec<AnyElement> {
    Vec::new()
}
