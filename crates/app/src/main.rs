//! The `tern` binary: parses flags and opens the window.

use gpui::{
    App, AppContext as _, Bounds, TitlebarOptions, WindowBounds, WindowDecorations, WindowOptions,
    px, size,
};

use tern::app_state::{AppState, Route};
use tern::theme::{Theme, ThemeMode};
use tern::{assets, shell, store};

struct Args {
    demo: bool,
    theme: ThemeMode,
    route: Route,
    layout: Vec<String>,
}

const USAGE: &str =
    "usage: tern [--demo] [--theme light|dark|system] [--route ROUTE] [--layout LIST]
routes: index, draft:<project-id>, thread:<thread-id>, settings, settings/<page>, gallery
layout (comma list): right-panel, maximized, terminal, palette, collapsed, no-details";

const LAYOUTS: [&str; 6] = [
    "right-panel",
    "maximized",
    "terminal",
    "palette",
    "collapsed",
    "no-details",
];

fn parse_args() -> Result<Args, String> {
    let mut args = Args {
        demo: false,
        theme: ThemeMode::System,
        route: Route::Index,
        layout: Vec::new(),
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
            "--layout" => {
                let v = value("--layout")?;
                for part in v.split(',').filter(|p| !p.is_empty()) {
                    if !LAYOUTS.contains(&part) {
                        return Err(format!("unknown layout {part}"));
                    }
                    args.layout.push(part.to_string());
                }
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
            let app = AppState::install(args.route.clone(), args.theme, cx);
            app.update(cx, |s, _| {
                for part in &args.layout {
                    match part.as_str() {
                        "right-panel" => s.right_panel.open = true,
                        "maximized" => {
                            s.right_panel.open = true;
                            s.right_panel_maximized = true;
                        }
                        "terminal" => s.drawer.open = true,
                        "palette" => s.palette_open = true,
                        "collapsed" => s.sidebar.open = false,
                        "no-details" => s.thread_details_open = false,
                        _ => {}
                    }
                }
            });
            let store = store::Store::install(args.demo, cx);
            // Demo thread ids are random, so `thread:<words>` also matches a title.
            if let Route::Thread(needle) = &args.route {
                let snapshot = store.read(cx).snapshot();
                let words = needle.to_lowercase().replace(['-', '_'], " ");
                let found = snapshot
                    .thread(needle)
                    .is_none()
                    .then(|| {
                        snapshot
                            .threads
                            .iter()
                            .find(|t| t.thread.title.to_lowercase().contains(&words))
                            .map(|t| t.thread.id.clone())
                    })
                    .flatten();
                if let Some(id) = found {
                    app.update(cx, |s, cx| s.navigate(Route::Thread(id), cx));
                }
            }

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
