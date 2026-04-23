//! Print every HID event to stderr so you can tune thresholds and confirm
//! parsing. Run with `cargo run --example pad_monitor`.

use futures::StreamExt;
use maschine_core::{Event, Maschine, PadPhase};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    let mk3 = Maschine::open().await?;
    let mut events = mk3.take_events().await.expect("events taken once");
    tracing::info!("listening for Mk3 input (Ctrl+C to exit)");
    while let Some(ev) = events.next().await {
        match ev {
            Event::Pad { pad, pressure, velocity, phase } => {
                let tag = match phase { PadPhase::Attack => "↘ATTACK", PadPhase::Pressure => "~PRESS ", PadPhase::Release => "↗REL   " };
                println!("pad {:>2} {tag} pressure=0x{pressure:04x} vel={velocity:?}", pad);
            }
            Event::Button { bit, pressed } => println!("btn bit={bit:2} {}", if pressed { "DOWN" } else { "UP" }),
            Event::MacroEncoder { index, delta, absolute } => println!("macro {index} Δ{delta:+} abs=0x{absolute:03x}"),
            Event::MasterEncoder { delta, absolute } => println!("master Δ{delta:+} abs=0x{absolute:03x}"),
            Event::TouchStrip { position, pressure } => println!("strip pos=0x{position:03x} pr=0x{pressure:03x}"),
            Event::TouchStripReleased => println!("strip RELEASE"),
            Event::Analog { which, value } => println!("{which:?}=0x{value:03x}"),
            Event::Raw(r) => println!("raw {r:?}"),
        }
    }
    Ok(())
}
