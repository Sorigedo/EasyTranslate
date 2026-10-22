use easytranslate::placement::{Rect, popup_origin, preview_origin};

#[test]
fn popup_opens_at_the_upper_right_of_the_visible_screen() {
    let screen = Rect::new(-1440., 20., 1440., 880.);
    assert_eq!(popup_origin(screen, 500., 470.), (-512., 422.));
}

#[test]
fn popup_stays_on_screen_when_the_screen_is_smaller() {
    let screen = Rect::new(100., 50., 420., 370.);
    assert_eq!(popup_origin(screen, 500., 470.), (100., 50.));
}

#[test]
fn preview_sits_above_the_chat_input() {
    let input = Rect::new(100., 750., 300., 40.);
    let screen = Rect::new(0., 0., 1024., 900.);
    assert_eq!(
        preview_origin(input, 900., screen, 510., 190.),
        (100., 158.)
    );
}

#[test]
fn preview_uses_space_below_when_input_is_near_screen_top() {
    let input = Rect::new(100., 80., 300., 40.);
    let screen = Rect::new(0., 0., 1024., 900.);
    assert_eq!(
        preview_origin(input, 900., screen, 510., 190.),
        (100., 582.)
    );
}

#[test]
fn preview_stays_inside_the_screen_horizontal_bounds() {
    let input = Rect::new(950., 750., 150., 40.);
    let screen = Rect::new(0., 0., 1024., 900.);
    assert_eq!(
        preview_origin(input, 900., screen, 510., 190.),
        (514., 158.)
    );
}
