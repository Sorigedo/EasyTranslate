#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Rect {
    pub const fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }
}

pub fn popup_origin(visible_screen: Rect, popup_width: f64, popup_height: f64) -> (f64, f64) {
    let x = (visible_screen.x + visible_screen.width - popup_width - 12.).max(visible_screen.x);
    let y = (visible_screen.y + visible_screen.height - popup_height - 8.).max(visible_screen.y);
    (x, y)
}

/// AX uses a top-left origin; AppKit windows use a bottom-left origin.
pub fn preview_origin(
    input: Rect,
    main_screen_top: f64,
    visible_screen: Rect,
    preview_width: f64,
    preview_height: f64,
) -> (f64, f64) {
    let input_top = main_screen_top - input.y;
    let input_bottom = input_top - input.height;
    let x = input.x.clamp(
        visible_screen.x,
        (visible_screen.x + visible_screen.width - preview_width).max(visible_screen.x),
    );
    let above = input_top + 8.;
    let below = input_bottom - preview_height - 8.;
    let screen_top = visible_screen.y + visible_screen.height;
    let preferred_y = if above + preview_height <= screen_top {
        above
    } else if below >= visible_screen.y {
        below
    } else {
        above
    };
    let y = preferred_y.clamp(
        visible_screen.y,
        (screen_top - preview_height).max(visible_screen.y),
    );
    (x, y)
}
