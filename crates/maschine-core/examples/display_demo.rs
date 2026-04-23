//! Animate both 480×272 displays: filled background + a bouncing rectangle
//! per screen, driven by the dirty-rect pipeline at 60fps.

use std::time::Duration;

use maschine_core::Maschine;
use maschine_core::display::{DisplayHandle, Framebuffer};
use maschine_proto::{DisplayId, Rgb};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    let mk3 = Maschine::open().await?;
    let transport = mk3.transport();

    let left = DisplayHandle::new(DisplayId::Left);
    let right = DisplayHandle::new(DisplayId::Right);
    let _left_task = left.clone().spawn(transport.clone(), 60);
    let _right_task = right.clone().spawn(transport.clone(), 60);

    let mut t = 0u32;
    loop {
        animate(&left, Rgb::new(0x10, 0x10, 0x30), Rgb::new(0xff, 0x80, 0x20), t).await;
        animate(&right, Rgb::new(0x10, 0x30, 0x10), Rgb::new(0x20, 0xc0, 0xff), t + 90).await;
        t = (t + 1) % 1000;
        tokio::time::sleep(Duration::from_millis(16)).await;
    }
}

async fn animate(handle: &DisplayHandle, bg: Rgb, fg: Rgb, t: u32) {
    let x = 20 + ((((t as f32) * 0.08).sin() + 1.0) * 200.0) as u16;
    let y = 20 + ((((t as f32) * 0.11).cos() + 1.0) * 110.0) as u16;
    handle.modify(|fb: &mut Framebuffer| {
        fb.clear(bg);
        fb.fill_rect(x, y, 64, 48, fg);
    }).await;
}
