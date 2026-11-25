//! Surface grain (spec 01 §1.8): the baked 256px fractal-noise tile, tiled over a
//! surface's fill. The tile already carries the 3.5% opacity in its alpha.
//!
//! Add `Grain` as the first child of a `relative()` surface (the body, the sidebar,
//! floating chrome with `surface-grain`).

use gpui::{
    App, Bounds, Corners, IntoElement, ParentElement as _, RenderOnce, Styled, Window, canvas, div,
    point, px, size,
};

use crate::assets::grain_tile;

#[derive(IntoElement, Default)]
pub struct Grain;

impl Grain {
    pub fn new() -> Self {
        Self
    }
}

/// CSS px of one tile.
const TILE: f32 = 256.;

impl RenderOnce for Grain {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        div().absolute().top_0().left_0().size_full().child(
            canvas(
                |_, _, _| {},
                |bounds, _, window, _| {
                    let Some(tile) = grain_tile(window.scale_factor()) else {
                        return;
                    };
                    // Tiles start at the window origin like a body background, so
                    // adjacent grained surfaces line up.
                    let start_x = (f32::from(bounds.origin.x) / TILE).floor() * TILE;
                    let start_y = (f32::from(bounds.origin.y) / TILE).floor() * TILE;
                    let end_x = f32::from(bounds.origin.x) + f32::from(bounds.size.width);
                    let end_y = f32::from(bounds.origin.y) + f32::from(bounds.size.height);
                    let mut y = start_y;
                    while y < end_y {
                        let mut x = start_x;
                        while x < end_x {
                            let tile_bounds =
                                Bounds::new(point(px(x), px(y)), size(px(TILE), px(TILE)));
                            let _ = window.paint_image(
                                bounds,
                                tile_bounds,
                                Corners::default(),
                                tile.clone(),
                                0,
                                false,
                            );
                            x += TILE;
                        }
                        y += TILE;
                    }
                },
            )
            .size_full(),
        )
    }
}
