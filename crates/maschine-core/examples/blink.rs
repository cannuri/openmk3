//! Cycle all 16 pads through a rainbow while flashing every button LED.
//!
//! Run with the Mk3 plugged in:
//! ```sh
//! cargo run --example blink
//! ```

use std::time::Duration;

use maschine_core::Maschine;
use maschine_proto::hid_out::BUTTON_LED_SLOTS;
use maschine_proto::{Rgb, PAD_COUNT};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    let mk3 = Maschine::open().await?;
    tracing::info!("Mk3 opened; cycling LEDs (Ctrl+C to exit)");

    let mut phase = 0u32;
    loop {
        let mut pads = [Rgb::BLACK; PAD_COUNT];
        for (i, p) in pads.iter_mut().enumerate() {
            *p = hsv_to_rgb(((phase + (i as u32) * 22) % 360) as u16, 255, 255);
        }
        let strip = [Rgb::BLACK; maschine_proto::TOUCHSTRIP_LED_COUNT];
        mk3.set_pads(&strip, &pads).await?;

        let mut btn = [0u8; BUTTON_LED_SLOTS];
        let lit = ((phase / 12) as usize) % BUTTON_LED_SLOTS;
        btn[lit] = 0x7f;
        mk3.set_button_leds(&btn).await?;

        phase = (phase + 6) % 360;
        tokio::time::sleep(Duration::from_millis(30)).await;
    }
}

fn hsv_to_rgb(h: u16, s: u8, v: u8) -> Rgb {
    let h = h % 360;
    let s = s as f32 / 255.0;
    let v = v as f32 / 255.0;
    let c = v * s;
    let hp = h as f32 / 60.0;
    let x = c * (1.0 - (hp % 2.0 - 1.0).abs());
    let (r1, g1, b1) = match hp as i32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = v - c;
    Rgb::new(
        ((r1 + m) * 255.0) as u8,
        ((g1 + m) * 255.0) as u8,
        ((b1 + m) * 255.0) as u8,
    )
}
