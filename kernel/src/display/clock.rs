use core::{f32::consts::PI, time::Duration};

use chrono::Timelike;
use embedded_graphics::{
    pixelcolor::Rgb888,
    prelude::*,
    primitives::{Circle, Line, PrimitiveStyle, PrimitiveStyleBuilder},
};
use libm::{cosf, sinf};

use crate::{framebuffer::DISPLAY, rtc::read_date_time, util::r#async::sleep};

const MARGIN: u32 = 50;

#[allow(unused_must_use)]
pub async fn draw_clock() {
    let mut disp = DISPLAY.get().lock().await;
    let target = disp.as_mut();
    let bounding_box = target.bounding_box();

    let diameter = bounding_box.size.width.min(bounding_box.size.height) - 2 * MARGIN;

    let clock_face = Circle::with_center(bounding_box.center(), diameter);
    target.clear(Rgb888::BLACK);
    draw_face(target, &clock_face);
    let mut last_time = read_date_time().time();
    drop(disp);
    let mut first = true;
    loop {
        let mut disp = DISPLAY.get().lock().await;
        let target = disp.as_mut();
        let time = read_date_time().time();
        //info!(%time);

        if time == last_time {
            sleep(Duration::from_millis(50)).await;
            continue;
        }
        // Calculate the position of the three clock hands in radians.
        let hours_radians = hour_to_angle(time.hour());
        let minutes_radians = sexagesimal_to_angle(time.minute());
        let seconds_radians = sexagesimal_to_angle(time.second());

        draw_hand(target, &clock_face, hours_radians, -60, Rgb888::WHITE);
        draw_hand(target, &clock_face, minutes_radians, -30, Rgb888::WHITE);
        draw_hand(target, &clock_face, seconds_radians, 0, Rgb888::WHITE);
        draw_second_decoration(target, &clock_face, seconds_radians, -20, Rgb888::WHITE);

        Circle::with_center(clock_face.center(), 9)
            .into_styled(PrimitiveStyle::with_fill(Rgb888::WHITE))
            .draw(target);

        if last_time.second() != time.second() {
            let seconds_radians = sexagesimal_to_angle(last_time.second());
            draw_hand(target, &clock_face, seconds_radians, 0, Rgb888::BLACK);
            draw_second_decoration(target, &clock_face, seconds_radians, -20, Rgb888::BLACK);

            draw_face(target, &clock_face);
        }
        if last_time.minute() != time.minute() {
            let minutes_radians = sexagesimal_to_angle(last_time.minute());
            draw_hand(target, &clock_face, minutes_radians, -30, Rgb888::BLACK);
        }
        if last_time.hour() != time.hour() {
            let hours_radians = hour_to_angle(last_time.hour());
            draw_hand(target, &clock_face, hours_radians, -60, Rgb888::BLACK);
        }
        sleep(Duration::from_millis(50)).await;

        last_time = time;
        if first {
            first = false;
        }
    }
}

fn polar(circle: &Circle, angle: f32, radius_delta: i32) -> Point {
    let radius = circle.diameter as f32 / 2.0 + radius_delta as f32;

    circle.center()
        + Point::new(
            (sinf(angle) * radius) as i32,
            -(cosf(angle) * radius) as i32,
        )
}
/// Converts an hour into an angle in radians.
fn hour_to_angle(hour: u32) -> f32 {
    // Convert from 24 to 12 hour time.
    let hour = hour % 12;

    (hour as f32 / 12.0) * 2.0 * PI
}
/// Converts a sexagesimal (base 60) value into an angle in radians.
fn sexagesimal_to_angle(value: u32) -> f32 {
    (value as f32 / 60.0) * 2.0 * PI
}

/// Draws a circle and 12 graduations as a simple clock face.
fn draw_face<D>(target: &mut D, clock_face: &Circle) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb888>,
{
    // Draw the outer face.
    (*clock_face)
        .into_styled(PrimitiveStyle::with_stroke(Rgb888::BLACK, 2))
        .draw(target)?;

    // Draw 12 graduations.
    for angle in (0..12).map(hour_to_angle) {
        // Start point on circumference.
        let start = polar(clock_face, angle, 0);

        // End point offset by 10 pixels from the edge.
        let end = polar(clock_face, angle, -10);

        Line::new(start, end)
            .into_styled(PrimitiveStyle::with_stroke(Rgb888::WHITE, 1))
            .draw(target)?;
    }

    Ok(())
}

/// Draws a clock hand.
fn draw_hand<D>(
    target: &mut D,
    clock_face: &Circle,
    angle: f32,
    length_delta: i32,
    color: Rgb888,
) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb888>,
{
    let end = polar(clock_face, angle, length_delta);

    Line::new(clock_face.center(), end)
        .into_styled(PrimitiveStyle::with_stroke(color, 1))
        .draw(target)
}

/// Draws a decorative circle on the second hand.
fn draw_second_decoration<D>(
    target: &mut D,
    clock_face: &Circle,
    angle: f32,
    length_delta: i32,
    color: Rgb888,
) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb888>,
{
    let decoration_position = polar(clock_face, angle, length_delta);

    let decoration_style = PrimitiveStyleBuilder::new()
        .fill_color(Rgb888::BLACK)
        .stroke_color(color)
        .stroke_width(1)
        .build();

    // Draw a fancy circle near the end of the second hand.
    Circle::with_center(decoration_position, 11)
        .into_styled(decoration_style)
        .draw(target)
}
