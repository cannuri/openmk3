//! End-to-end pipeline tests that don't need a real device. They exercise
//! the framebuffer + dirty tracker + command encoder stack by inspecting
//! the bytes the transport *would* emit.

use maschine_core::display::{encoder, Framebuffer};
use maschine_proto::{DisplayId, Rgb};

#[test]
fn clear_then_partial_update_emits_two_command_blocks() {
    let mut fb = Framebuffer::new();
    fb.clear(Rgb::BLACK);
    // After clear, the dirty tracker marks every tile. Encode the whole
    // frame and assert it closes with flush+end.
    let full = encoder::encode_frame(DisplayId::Left, &mut fb);
    assert!(full.len() > 32);
    assert_eq!(full[full.len() - 16], 0x03); // flush
    assert_eq!(full[full.len() - 8], 0x40);  // end

    // Tracker cleared — next encode with no touch() emits only flush+end.
    let empty = encoder::encode_frame(DisplayId::Left, &mut fb);
    assert_eq!(empty.len(), 16);

    // Touch a tiny rect and encode again: one command block + flush + end.
    fb.fill_rect(48, 48, 16, 16, Rgb::new(0xff, 0, 0));
    let small = encoder::encode_frame(DisplayId::Left, &mut fb);
    assert!(small.len() >= 32 + 8 + 16);
    // Header starts with 0x84; solid fill → repeat opcode 0x01 right after.
    assert_eq!(small[0], 0x84);
    assert_eq!(small[32], 0x01);
}

#[test]
fn solid_fill_compresses_to_repeat() {
    let mut fb = Framebuffer::new();
    // Four adjacent tiles all one color should collapse to one repeat run.
    fb.fill_rect(16, 16, 64, 16, Rgb::new(0, 0xff, 0));
    let bytes = encoder::encode_frame(DisplayId::Right, &mut fb);
    // Header (32) + one repeat cmd (8) + flush (8) + end (8) = 56.
    assert_eq!(bytes.len(), 56);
    assert_eq!(bytes[32], 0x01);
}
