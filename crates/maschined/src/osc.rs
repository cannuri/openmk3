//! OSC over UDP — event fan-out + command intake.
//!
//! Listens on `127.0.0.1:57130` by default. Clients send commands to the
//! same port; events are fanned out to every sender that has transmitted
//! to us within the last minute (poor man's subscription).

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use rosc::{encoder, OscMessage, OscPacket, OscType};
use tokio::net::UdpSocket;
use tokio::sync::{mpsc, Mutex};

const SUBSCRIBER_TTL: Duration = Duration::from_secs(60);

pub type Command = (SocketAddr, OscMessage);

/// Start the OSC listener. Returns:
///   * an inbound `Command` receiver (for the session manager to consume)
///   * a `broadcast` closure that ships outbound OSC to every recent sender
pub async fn serve(
    bind: SocketAddr,
) -> Result<(mpsc::Receiver<Command>, Arc<OscBroadcaster>)> {
    let sock = Arc::new(UdpSocket::bind(bind).await?);
    let subs: Arc<Mutex<HashMap<SocketAddr, Instant>>> = Arc::new(Mutex::new(HashMap::new()));
    let (cmd_tx, cmd_rx) = mpsc::channel::<Command>(256);

    let bcast = Arc::new(OscBroadcaster { sock: sock.clone(), subs: subs.clone() });

    // Recv task
    {
        let sock = sock.clone();
        let subs = subs.clone();
        tokio::spawn(async move {
            let mut buf = [0u8; 2048];
            loop {
                let Ok((n, src)) = sock.recv_from(&mut buf).await else { break };
                subs.lock().await.insert(src, Instant::now());
                let Ok((_rest, pkt)) = rosc::decoder::decode_udp(&buf[..n]) else { continue };
                if let OscPacket::Message(m) = pkt {
                    let _ = cmd_tx.send((src, m)).await;
                }
            }
        });
    }

    Ok((cmd_rx, bcast))
}

pub struct OscBroadcaster {
    sock: Arc<UdpSocket>,
    subs: Arc<Mutex<HashMap<SocketAddr, Instant>>>,
}

impl OscBroadcaster {
    pub async fn emit(&self, addr: &str, args: Vec<OscType>) {
        let pkt = OscPacket::Message(OscMessage { addr: addr.into(), args });
        let Ok(buf) = encoder::encode(&pkt) else { return };
        let mut subs = self.subs.lock().await;
        let now = Instant::now();
        subs.retain(|_, seen| now.duration_since(*seen) < SUBSCRIBER_TTL);
        for dst in subs.keys() {
            let _ = self.sock.send_to(&buf, dst).await;
        }
    }
}
