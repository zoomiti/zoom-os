use core::{f32::consts::PI, time::Duration};

use alloc::format;
use chrono::Timelike;
use embedded_graphics::{
    mono_font::{ascii::FONT_9X15, MonoTextStyle},
    pixelcolor::Rgb888,
    prelude::*,
    primitives::{Circle, Line, PrimitiveStyle, PrimitiveStyleBuilder, Rectangle},
    text::Text,
};
use libm::{cosf, sinf};
use tracing::info;

use crate::{framebuffer::DISPLAY, rtc::RTC, util::r#async::sleep};

const MARGIN: u32 = 50;

#[tracing::instrument]
#[allow(unused_must_use)]
pub async fn draw_clock() {
    let clock_face = {
        let mut disp = DISPLAY.get().lock().await;
        let target = disp.as_mut();
        let bounding_box = target.bounding_box();

        let diameter = bounding_box.size.width.min(bounding_box.size.height) - 2 * MARGIN;

        target.clear(Rgb888::BLACK);
        Circle::with_center(bounding_box.center(), diameter)
    };
    let center_clock_face = Circle::with_center(clock_face.center(), 9)
        .into_styled(PrimitiveStyle::with_fill(Rgb888::WHITE));
    let mut last_time = RTC.lock().await.read_date_time().time();
    loop {
        let time = RTC.lock().await.read_date_time().time();

        if time == last_time {
            sleep(Duration::from_millis(50)).await;
            continue;
        }
        info!("{}", time);

        let digital_clock_text = format!(
            "{:02}:{:02}:{:02}",
            time.hour(),
            time.minute(),
            time.second()
        );

        // Calculate the position of the three clock hands in radians.
        let hours_radians = hour_to_angle(time.hour());
        let minutes_radians = sexagesimal_to_angle(time.minute());
        let seconds_radians = sexagesimal_to_angle(time.second());

        {
            let mut disp = DISPLAY.get().lock().await;
            let target = disp.as_mut();
            target.clear(Rgb888::BLACK);

            draw_face(target, &clock_face);

            draw_hand(target, &clock_face, hours_radians, -60, Rgb888::WHITE);
            draw_hand(target, &clock_face, minutes_radians, -30, Rgb888::WHITE);
            draw_hand(target, &clock_face, seconds_radians, 0, Rgb888::WHITE);
            draw_second_decoration(target, &clock_face, seconds_radians, -20, Rgb888::WHITE);

            draw_digital_clock(target, &clock_face, &digital_clock_text);

            center_clock_face.draw(target);

            target.draw_frame();
        }
        sleep(Duration::from_millis(50)).await;

        last_time = time;
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

/// Draw digital clock just above center with black text on a white background
fn draw_digital_clock<D>(
    target: &mut D,
    clock_face: &Circle,
    time_str: &str,
) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb888>,
{
    // Create a styled text object for the time text.
    let mut text = Text::new(
        &time_str,
        Point::zero(),
        MonoTextStyle::new(&FONT_9X15, Rgb888::BLACK),
    );

    // Move text to be centered between the 12 o'clock point and the center of the clock face.
    text.translate_mut(
        clock_face.center()
            - text.bounding_box().center()
            - clock_face.bounding_box().size.y_axis() / 4,
    );

    // Add a background around the time digits.
    // Note that there is no bottom-right padding as this is added by the font renderer itself.
    let text_dimensions = text.bounding_box();
    Rectangle::new(
        text_dimensions.top_left - Point::new(3, 3),
        text_dimensions.size + Size::new(4, 4),
    )
    .into_styled(PrimitiveStyle::with_fill(Rgb888::WHITE))
    .draw(target)?;

    // Draw the text after the background is drawn.
    text.draw(target)?;

    Ok(())
}
