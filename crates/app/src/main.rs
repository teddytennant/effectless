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

mod app_state;
mod assets;
mod fixtures;
mod gallery;
mod shell;
mod store;
mod theme;
mod tokens;
mod ui;
mod views;

use gpui::{
    App, AppContext as _, Bounds, TitlebarOptions, WindowBounds, WindowDecorations, WindowOptions,
    px, size,
};

use app_state::{AppState, Route};
use theme::{Theme, ThemeMode};

struct Args {
    demo: bool,
    theme: ThemeMode,
    route: Route,
}

const USAGE: &str = "usage: tern [--demo] [--theme light|dark|system] [--route ROUTE]
routes: index, draft:<project-id>, thread:<thread-id>, settings, settings/<page>, gallery";

fn parse_args() -> Result<Args, String> {
    let mut args = Args {
        demo: false,
        theme: ThemeMode::System,
        route: Route::Index,
    };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        let (flag, inline) = match arg.split_once('=') {
            Some((f, v)) => (f.to_string(), Some(v.to_string())),
            None => (arg.clone(), None),
        };
        let mut value = |name: &str| {
            inline
                .clone()
                .or_else(|| it.next())
                .ok_or_else(|| format!("{name} needs a value"))
        };
        match flag.as_str() {
            "--demo" => args.demo = true,
            "--theme" => {
                let v = value("--theme")?;
                args.theme = ThemeMode::parse(&v).ok_or_else(|| format!("unknown theme {v}"))?;
            }
            "--route" => {
                let v = value("--route")?;
                args.route = Route::parse(&v).ok_or_else(|| format!("unknown route {v}"))?;
            }
            "-h" | "--help" => return Err(String::new()),
            other => return Err(format!("unknown argument {other}")),
        }
    }
    Ok(args)
}

fn main() {
    let args = match parse_args() {
        Ok(args) => args,
        Err(err) => {
            if !err.is_empty() {
                eprintln!("tern: {err}");
            }
            eprintln!("{USAGE}");
            std::process::exit(2);
        }
    };

    gpui_platform::application()
        .with_assets(assets::Assets)
        .run(move |cx: &mut App| {
            assets::load_fonts(cx);
            gpui_component::init(cx);
            shell::bind_keys(cx);
            AppState::install(args.route.clone(), args.theme, cx);
            store::Store::install(args.demo, cx);

            // spec 02 §1.1: 1100 x 780 default, 400 x 360 minimum.
            let bounds = Bounds::centered(None, size(px(1100.), px(780.)), cx);
            let options = WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                window_min_size: Some(size(px(400.), px(360.))),
                titlebar: Some(TitlebarOptions {
                    title: Some("Tern".into()),
                    appears_transparent: true,
                    traffic_light_position: None,
                }),
                window_decorations: Some(WindowDecorations::Client),
                app_id: Some("tern".into()),
                ..Default::default()
            };
            cx.open_window(options, |window, cx| {
                Theme::init(args.theme, window, cx);
                let shell = cx.new(|cx| shell::Shell::new(window, cx));
                cx.new(|cx| gpui_component::Root::new(shell, window, cx))
            })
            .expect("failed to open the window");
            cx.activate(true);
        });
}
