//! `maschined` — owns the Mk3 device and serves OSC/WS/UDS clients.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use base64::Engine;
use futures::StreamExt;
use maschine_core::{Event, Maschine, PadPhase};
use maschine_core::display::{DisplayHandle, Framebuffer};
use maschine_proto::{DisplayId, Rgb};
use maschine_ui::{BrowseState, PixelSink};
use nks_index::{default_roots, query, Scanner};
use nks_parse::NksFile;
use plugin_registry::Registry;
use tokio::sync::{mpsc, Mutex};

mod osc;
mod ws;
mod session;
mod ipc;

use session::SessionManager;

/// Adapter that lets maschine-ui render straight into a maschine-core framebuffer.
struct FbSink<'a>(&'a mut Framebuffer);
impl<'a> PixelSink for FbSink<'a> {
    fn width(&self) -> u16 { self.0.width() as u16 }
    fn height(&self) -> u16 { self.0.height() as u16 }
    fn set(&mut self, x: u16, y: u16, c: Rgb) { self.0.set_pixel(x, y, c); }
}

/// Shared application state.
struct App {
    browse: Mutex<BrowseState>,
    registry: Registry,
    scanner: Mutex<Scanner>,
    sessions: Arc<SessionManager>,
    left: DisplayHandle,
    right: DisplayHandle,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let osc_bind: SocketAddr = "127.0.0.1:57130".parse().unwrap();
    let uds_path = runtime_dir().join("maschined.sock");
    tracing::info!(%osc_bind, uds = %uds_path.display(), "maschined starting (v0.1.0, macOS-only)");

    // Open the Mk3.
    let mk3 = Maschine::open().await?;
    let transport = mk3.transport();

    // Start display tasks at 60fps.
    let left = DisplayHandle::new(DisplayId::Left);
    let right = DisplayHandle::new(DisplayId::Right);
    let _left_task = left.clone().spawn(transport.clone(), 60);
    let _right_task = right.clone().spawn(transport.clone(), 60);

    // Open the index (default path under the user's state dir).
    let state_dir = state_dir();
    std::fs::create_dir_all(&state_dir).ok();
    let db_path = state_dir.join("index.sqlite");
    let scanner = Scanner::open(&db_path).with_context(|| format!("open {}", db_path.display()))?;

    // Scan all detected NKS plugins once at startup.
    let registry = Registry::scan();
    tracing::info!(plugins = registry.len(), "plugin registry scanned");

    // Plugin host binary — we expect it alongside the daemon in installed
    // builds; for dev, point to the CMake build dir via env var.
    let plugin_bin = plugin_host_path();
    let (sessions, mut plugin_events) = SessionManager::new(plugin_bin);

    // Initial browse state + render.
    let mut initial = BrowseState {
        facet_values: vec!["Synth".into(), "Bass".into(), "Lead".into(), "Pad".into(),
                           "Keys".into(), "Drums".into(), "Percussion".into(), "FX".into()],
        facet_cursor: 0,
        ..Default::default()
    };
    // Populate rows with the first 50 presets from the index, if any.
    {
        let db = rusqlite::Connection::open(&db_path)?;
        if let Ok(rows) = query::run(&db, &query::Query { limit: Some(50), ..Default::default() }) {
            initial.set_rows(rows);
        }
    }
    repaint(&left, &right, &initial).await;

    let app = Arc::new(App {
        browse: Mutex::new(initial),
        registry,
        scanner: Mutex::new(scanner),
        sessions,
        left: left.clone(),
        right: right.clone(),
    });

    // OSC listener + broadcaster.
    let (mut cmd_rx, bcast) = osc::serve(osc_bind).await?;

    // UDS listener for msctl.
    let (op_tx, mut op_rx) = mpsc::channel::<(ipc::Op, mpsc::Sender<ipc::Reply>)>(64);
    ipc::serve(uds_path.clone(), op_tx).await?;

    // Device events → OSC + browse-UI navigation.
    let bcast_events = bcast.clone();
    let app_for_events = app.clone();
    let mut events = mk3.take_events().await.expect("events available");
    tokio::spawn(async move {
        while let Some(ev) = events.next().await {
            broadcast_event(&bcast_events, &ev).await;
            handle_nav_event(&app_for_events, &ev).await;
        }
    });

    // OSC command pump.
    let mk3_for_osc = mk3_handle_arc(&mk3);
    tokio::spawn(async move {
        while let Some((_src, msg)) = cmd_rx.recv().await {
            if let Err(e) = handle_osc_cmd(&mk3_for_osc, &msg).await {
                tracing::debug!(addr = %msg.addr, "osc cmd failed: {e}");
            }
        }
    });

    // Kick off a library scan in the background so first-run users don't
    // stare at an empty browser. Re-scanning via /msctl browse picks up
    // anything added later; live FS watching is tracked as a v0.2 polish.
    let app_for_scan = app.clone();
    let db_for_scan = db_path.clone();
    tokio::spawn(async move {
        for root in default_roots() {
            let mut s = app_for_scan.scanner.lock().await;
            if let Ok(stats) = s.scan_root(&root) {
                tracing::info!(?stats, root = %root.display(), "background scan");
            }
        }
        // Refresh the initial browse view with whatever we found.
        if let Ok(db) = rusqlite::Connection::open(&db_for_scan) {
            if let Ok(rows) = query::run(&db, &query::Query { limit: Some(100), ..Default::default() }) {
                let mut b = app_for_scan.browse.lock().await;
                b.set_rows(rows);
                repaint(&app_for_scan.left, &app_for_scan.right, &b).await;
            }
        }
    });

    // Plugin-host event pump — currently just log.
    tokio::spawn(async move {
        while let Some(env) = plugin_events.recv().await {
            tracing::debug!(kind = %env.kind, "pluginhost event");
        }
    });

    // UDS op pump — terminates on Ctrl-C / SIGTERM so the transport's Drop
    // runs and the NI agent gets restored cleanly.
    let mut sigint = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())?;
    let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    loop {
        tokio::select! {
            Some((op, reply)) = op_rx.recv() => {
                let r = handle_uds_op(&app, &db_path, op).await;
                let _ = reply.send(r).await;
            }
            _ = sigint.recv() => {
                tracing::info!("SIGINT received — shutting down");
                break;
            }
            _ = sigterm.recv() => {
                tracing::info!("SIGTERM received — shutting down");
                break;
            }
            else => break,
        }
    }
    drop(mk3); // runs Transport::Drop → ClaimGuard::Drop → SIGCONT agents
    tracing::info!("maschined stopped cleanly");
    Ok(())
}

async fn handle_uds_op(app: &Arc<App>, db_path: &std::path::Path, op: ipc::Op) -> ipc::Reply {
    match op {
        ipc::Op::Status => {
            let browse = app.browse.lock().await;
            ipc::Reply { ok: true, payload: serde_json::json!({
                "version": env!("CARGO_PKG_VERSION"),
                "device": "Maschine Mk3",
                "plugins": app.registry.len(),
                "presets_loaded": browse.rows.len(),
            })}
        }
        ipc::Op::ScanLibraries => {
            let mut total = 0usize;
            for root in default_roots() {
                let mut s = app.scanner.lock().await;
                match s.scan_root(&root) {
                    Ok(st) => { total += st.added + st.updated; tracing::info!(?st, root = %root.display(), "scanned"); }
                    Err(e) => tracing::warn!(root = %root.display(), "scan failed: {e}"),
                }
            }
            ipc::Reply { ok: true, payload: serde_json::json!({"presets_changed": total}) }
        }
        ipc::Op::Browse(args) => {
            let db = match rusqlite::Connection::open(db_path) {
                Ok(d) => d,
                Err(e) => return ipc::Reply { ok: false, payload: serde_json::json!({"error": e.to_string()})},
            };
            let q = query::Query {
                text: args.text,
                vendor: args.vendor,
                type_filter: args.r#type,
                limit: args.limit.or(Some(100)),
            };
            match query::run(&db, &q) {
                Ok(rows) => {
                    let mut b = app.browse.lock().await;
                    b.set_rows(rows.clone());
                    repaint(&app.left, &app.right, &b).await;
                    ipc::Reply { ok: true, payload: serde_json::json!(rows) }
                }
                Err(e) => ipc::Reply { ok: false, payload: serde_json::json!({"error": e.to_string()}) },
            }
        }
        ipc::Op::Load { preset_id } => {
            let browse = app.browse.lock().await;
            let Some(row) = browse.rows.iter().find(|r| r.id == preset_id).cloned() else {
                return ipc::Reply { ok: false, payload: serde_json::json!({"error":"preset not in current view"}) };
            };
            drop(browse);
            let res = load_preset(app, &row).await;
            match res {
                Ok(info) => ipc::Reply { ok: true, payload: info },
                Err(e) => ipc::Reply { ok: false, payload: serde_json::json!({"error": e.to_string()}) },
            }
        }
    }
}

async fn load_preset(app: &Arc<App>, row: &nks_index::PresetRow) -> Result<serde_json::Value> {
    let nks = NksFile::scan(&row.path)?;
    let Some(entry) = app.registry.resolve(&nks.plugin) else {
        anyhow::bail!("no installed plugin for {:?}", nks.plugin);
    };
    let bundle = match entry {
        plugin_registry::PluginEntry::Vst3(v) => v.bundle.clone(),
        plugin_registry::PluginEntry::AudioUnit(a) => {
            anyhow::bail!("AU loading via pluginhost is M5.2 work; plugin: {}", a.name);
        }
    };
    let state = nks.read_state()?;
    let state_b64 = base64::engine::general_purpose::STANDARD.encode(&state);
    let env = app.sessions.load(&bundle, &state_b64).await?;
    Ok(serde_json::json!({ "bundle": bundle.display().to_string(), "pluginhost": env.body }))
}

async fn handle_nav_event(app: &Arc<App>, ev: &Event) {
    let mut changed = false;
    {
        let mut b = app.browse.lock().await;
        match ev {
            Event::MacroEncoder { index: 2, delta, .. } => {
                for _ in 0..(delta.unsigned_abs() as usize) {
                    if *delta > 0 { b.cursor_down(); } else { b.cursor_up(); }
                }
                changed = true;
            }
            Event::MacroEncoder { index: 0, delta, .. } => {
                let n = b.facet_values.len();
                if n > 0 {
                    let step = delta.signum() as isize;
                    let new = (b.facet_cursor as isize + step).rem_euclid(n as isize);
                    b.facet_cursor = new as usize;
                    changed = true;
                }
            }
            _ => {}
        }
    }
    if changed {
        let browse = app.browse.lock().await;
        repaint(&app.left, &app.right, &browse).await;
    }
}

async fn repaint(left: &DisplayHandle, right: &DisplayHandle, state: &BrowseState) {
    left.modify(|fb| state.render(DisplayId::Left, &mut FbSink(fb))).await;
    right.modify(|fb| state.render(DisplayId::Right, &mut FbSink(fb))).await;
}

fn mk3_handle_arc(m: &Maschine) -> Arc<maschine_core::transport::Transport> {
    m.transport()
}

async fn handle_osc_cmd(
    _transport: &Arc<maschine_core::transport::Transport>,
    msg: &rosc::OscMessage,
) -> Result<()> {
    // Stub: LED and display-image verbs will land once we have a stable color
    // palette mapping reverse-engineered from real hardware. Keeping the
    // dispatch table here so external clients can probe what's supported.
    match msg.addr.as_str() {
        "/mk3/led/pad" | "/mk3/led/button" | "/mk3/display/0/image" | "/mk3/display/1/image" => {
            tracing::debug!(addr = %msg.addr, "osc verb reserved (v0.2)");
        }
        _ => {}
    }
    Ok(())
}

fn runtime_dir() -> PathBuf {
    if let Ok(xdg) = std::env::var("XDG_RUNTIME_DIR") { return PathBuf::from(xdg); }
    PathBuf::from("/tmp")
}

fn state_dir() -> PathBuf {
    if let Ok(home) = std::env::var("HOME") {
        return PathBuf::from(home).join("Library/Application Support/maschined");
    }
    PathBuf::from("/tmp/maschined")
}

fn plugin_host_path() -> PathBuf {
    if let Ok(p) = std::env::var("MASCHINE_PLUGINHOST") {
        return PathBuf::from(p);
    }
    PathBuf::from("pluginhost/build/maschine-pluginhost_artefacts/maschine-pluginhost")
}

async fn broadcast_event(bcast: &Arc<osc::OscBroadcaster>, ev: &Event) {
    use rosc::OscType::*;
    match ev {
        Event::Pad { pad, pressure, velocity, phase } => {
            let addr = match phase {
                PadPhase::Attack => format!("/mk3/pad/{pad}/down"),
                PadPhase::Pressure => format!("/mk3/pad/{pad}/pressure"),
                PadPhase::Release => format!("/mk3/pad/{pad}/up"),
            };
            let mut args = vec![Int(*pressure as i32)];
            if let Some(v) = velocity { args.push(Int(*v as i32)); }
            bcast.emit(&addr, args).await;
        }
        Event::Button { bit, pressed } =>
            bcast.emit(&format!("/mk3/button/{bit}/{}", if *pressed {"down"} else {"up"}), vec![]).await,
        Event::MacroEncoder { index, delta, absolute } =>
            bcast.emit(&format!("/mk3/encoder/macro/{index}"), vec![Int(*delta as i32), Int(*absolute as i32)]).await,
        Event::MasterEncoder { delta, absolute } =>
            bcast.emit("/mk3/encoder/master", vec![Int(*delta as i32), Int(*absolute as i32)]).await,
        Event::TouchStrip { position, pressure } =>
            bcast.emit("/mk3/touchstrip", vec![Int(*position as i32), Int(*pressure as i32)]).await,
        Event::TouchStripReleased => bcast.emit("/mk3/touchstrip/release", vec![]).await,
        Event::Analog { which, value } =>
            bcast.emit(&format!("/mk3/analog/{which:?}"), vec![Int(*value as i32)]).await,
        Event::Raw(_) => {}
    }
}
