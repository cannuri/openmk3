//! Async display pipeline: double-buffered framebuffer, 16×16 dirty-tile
//! tracker, command-stream encoder, bulk-transfer scheduler.

pub mod framebuffer;
pub mod dirty;
pub mod encoder;

pub use framebuffer::Framebuffer;
pub use dirty::DirtyTracker;

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{Mutex, Notify};

use maschine_proto::DisplayId;

use crate::transport::Transport;

/// One display's driving state: framebuffer + dirty set + wake signal.
#[derive(Clone)]
pub struct DisplayHandle {
    id: DisplayId,
    fb: Arc<Mutex<Framebuffer>>,
    notify: Arc<Notify>,
}

impl DisplayHandle {
    /// Create a fresh handle with an empty framebuffer.
    pub fn new(id: DisplayId) -> Self {
        Self {
            id,
            fb: Arc::new(Mutex::new(Framebuffer::new())),
            notify: Arc::new(Notify::new()),
        }
    }

    pub fn id(&self) -> DisplayId {
        self.id
    }

    /// Run a closure that mutates the framebuffer. The closure must mark any
    /// modified regions via [`Framebuffer::touch`] — otherwise the next flush
    /// will skip them.
    pub async fn modify<R>(&self, f: impl FnOnce(&mut Framebuffer) -> R) -> R {
        let mut fb = self.fb.lock().await;
        let r = f(&mut *fb);
        self.notify.notify_one();
        r
    }

    /// Spawn the scheduler task driving this display. Consumes `self`; keep
    /// a clone if you still need to `modify()` the framebuffer afterwards.
    pub fn spawn(self, transport: Arc<Transport>, fps: u32) -> tokio::task::JoinHandle<()> {
        let fb = self.fb.clone();
        let notify = self.notify.clone();
        let id = self.id;
        let period = Duration::from_micros((1_000_000 / fps.max(1)) as u64);
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(period);
            tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                tokio::select! {
                    _ = notify.notified() => {}
                    _ = tick.tick() => {}
                }
                let payload_opt = {
                    let mut f = fb.lock().await;
                    if !f.is_dirty() {
                        None
                    } else {
                        Some(encoder::encode_frame(id, &mut *f))
                    }
                };
                if let Some(payload) = payload_opt {
                    if let Err(e) = transport.write_display(payload).await {
                        tracing::warn!("display {:?} write failed: {e}", id);
                        tokio::time::sleep(Duration::from_millis(20)).await;
                    }
                }
            }
        })
    }
}
