use gpui::{App, AppContext, Bounds, Context, Window, WindowBounds, WindowOptions, div, prelude::*, px, size};
use gpui_component::{Root, input::{Input, InputState}};

struct Hello { input: gpui::Entity<InputState> }

impl Render for Hello {
    fn render(&mut self, _w: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().size_full().p_4().child("tern").child(Input::new(&self.input))
    }
}

fn main() {
    gpui_platform::application().run(|cx: &mut App| {
        gpui_component::init(cx);
        let bounds = Bounds::centered(None, size(px(1200.), px(800.)), cx);
        cx.open_window(WindowOptions { window_bounds: Some(WindowBounds::Windowed(bounds)), ..Default::default() }, |window, cx| {
            let input = cx.new(|cx| InputState::new(window, cx).placeholder("Ask anything"));
            let view = cx.new(|_| Hello { input });
            cx.new(|cx| Root::new(view, window, cx))
        }).unwrap();
    });
}
