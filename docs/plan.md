# Maschine Mk3 — Full Native Host with Preset Browsing & Display Support

## Context

Native Instruments ships the Maschine Mk3 as a closed system: rich hardware (16 RGB pads with pressure, 8 endless encoders, touch strip, 2×480×272 color displays) usable only through their proprietary Maschine/Komplete Kontrol software. Without that software the device is a limited class-compliant MIDI surface — no displays, no rich feedback, no library browser. Phase 1 research verified that the community has reverse-engineered enough of the USB and NKS protocols to build a **fully open, cross-platform host** that gives programmatic control of every hardware surface *and* browses / loads the user's installed NI (and third-party NKS) instruments. This plan is the design to build it.

The project is greenfield — `/Users/tonic/Nerd/maschine` is an empty git repo — so we are not conforming to existing code.

**Decisions locked**
- Language: **Rust** for the daemon + all Rust crates; **C++/JUCE** subprocess for plugin hosting.
- Scope for v0.1: **Full** — hardware + displays + NKS parser + library index + plugin hosting + browse/load UI + OSC/WS API.
- Platform for v0.1: **macOS only.** Linux and Windows follow in v0.2; platform abstraction exists from day one but non-macOS backends are stubs that return `Error::UnsupportedPlatform` until then.

**Outcome targets**
- Zero dependency on NI's NIHostIntegrationAgent at runtime (we stop it on macOS, we never run alongside it)
- Every hardware surface driven: pads + velocity/pressure, encoders, buttons, touch strip, all LEDs, both 480×272 color displays
- Browse `.nksf` presets across the entire installed NI library + third-party NKS plugins
- Load any browsed preset into a hosted VST3/AU instance and route MIDI from the pads into it
- Programmable from Rust (library), from the shell (CLI), and over OSC / WebSocket (any language)

## Recommended Approach

A single Rust workspace containing a daemon (`maschined`) that owns the USB device and library index, plus a C++/JUCE subprocess per loaded plugin. External clients talk OSC+WebSocket.

```
┌──────────────────────────────────────────────────────────────┐
│                         maschined (Rust)                     │
│  ┌─────────────┐  ┌──────────────┐  ┌──────────────────┐     │
│  │ USB/HID     │  │ Display      │  │ Event Bus        │     │
│  │ transport   │─▶│ framebuffers │─▶│ (broadcast)      │     │
│  │ (nusb)      │  │ + dirty rect │  │                  │     │
│  └─────────────┘  └──────────────┘  └───┬──────────────┘     │
│                                         │                    │
│  ┌─────────────┐  ┌──────────────┐  ┌───▼──────────────┐     │
│  │ NKS parser  │─▶│ SQLite index │─▶│ Browse/Load      │     │
│  │ (riff+msgpk)│  │ + FTS5       │  │ state machine    │     │
│  └─────────────┘  └──────────────┘  └───┬──────────────┘     │
│                                         │                    │
│  ┌──────────────────┐    ┌──────────────▼───────────────┐    │
│  │ OSC + WebSocket  │◀──▶│ Plugin session manager       │    │
│  │ + UDS (msctl)    │    │ (spawns maschine-pluginhost) │    │
│  └──────────────────┘    └──────────────┬───────────────┘    │
└─────────────────────────────────────────┼────────────────────┘
                                          │ UDS + shm
                              ┌───────────▼─────────────────┐
                              │ maschine-pluginhost (C++/   │
                              │ JUCE) — one process per     │
                              │ loaded plugin, sandboxed    │
                              └─────────────────────────────┘
```

### Why this combination

1. **Direct USB over NIHIA** — NIHIA (NI's IPC) is macOS/Windows only, undocumented, ToS-gray, exclusive-access-unclear, and crucially **does not expose preset browsing commands** (verified from the rebellion source). Direct USB works on all three OSes, survives NI updates, and community protocol docs (Drachenkaetzchen cabl fork, ktemkin gist) are complete enough to implement from.
2. **Subprocess plugin hosting** — a bad plugin will eventually crash; isolating each plugin to its own process keeps `maschined` alive and the hardware responsive. This is what Ableton and REAPER do for the same reason.
3. **JUCE over rolling our own** — JUCE's `AudioPluginFormatManager` + `AudioPluginInstance` handles VST3/AU/VST2 correctly; writing our own is multi-month duplicate work. Commercial/GPL dual license is workable.
4. **OSC+WebSocket external API** — lingua franca of music tooling, zero-install for browser UIs, no protobuf/gRPC surprise dependencies leaking into Max/Pd/SuperCollider embedders.

### Crate layout (Rust workspace)

```
/Users/tonic/Nerd/maschine/
├── Cargo.toml                    # [workspace]
├── crates/
│   ├── maschine-proto/           # pure parsers/encoders, no I/O
│   │   └── src/{hid_in.rs, hid_out.rs, display.rs, types.rs, lib.rs}
│   ├── maschine-core/            # USB transport, event loop, display pipeline
│   │   └── src/{transport.rs, device.rs, event.rs, display/, platform/, lib.rs}
│   ├── nks-parse/                # .nksf RIFF + msgpack parser
│   │   └── src/{riff.rs, nisi.rs, plid.rs, nica.rs, lib.rs}
│   ├── nks-index/                # filesystem scanner + SQLite/FTS5 store
│   │   └── src/{scanner.rs, schema.sql, query.rs, watch.rs, lib.rs}
│   ├── plugin-registry/          # VST3/AU scan + PLID→plugin resolution
│   │   └── src/{vst3.rs, au.rs, resolve.rs, lib.rs}
│   ├── maschine-ui/              # hardware UI model (display framebuffers, nav state)
│   │   └── src/{browse.rs, layout.rs, lib.rs}
│   └── maschined/                # daemon binary (OSC, WS, UDS, session mgr)
│       └── src/{main.rs, osc.rs, ws.rs, session.rs, ipc.rs}
├── pluginhost/                   # C++/JUCE child process
│   ├── CMakeLists.txt
│   └── src/{main.cpp, ipc.cpp, session.cpp}
├── msctl/                        # CLI client (Rust)
├── resources/
│   └── macos/launchd.plist
│   # (99-maschine.rules for Linux added in v0.2)
└── examples/
    ├── blink.rs
    ├── pad_monitor.rs
    ├── display_demo.rs
    └── embedded_graphics.rs
```

### Key technical decisions

**USB transport**: `nusb` (pure-Rust, async, native IOKit/WinUSB/usbfs backends). Handles both HID interface #4 (controls + LEDs) and vendor bulk interface #5 (displays) from a single device handle — avoids the two-handle conflict that plagues hidapi+libusb combos on macOS.

**Display pipeline**: 480×272 RGB565 framebuffers (`Vec<u16>` native-endian for CPU ops, byte-swapped to big-endian during transfer). 16×16 tile dirty tracker (`[u64; 8]` bitmap). Greedy row-wise dirty-tile merging → command stream of 32-byte header + 8-byte chunks (`0x00` blit / `0x01` repeat / `0x03` flush / `0x40` end) → single bulk OUT on endpoint `0x04`. Opportunistic `0x01` repeat command for solid fills. Target 30fps default, 60fps ceiling for small dirty regions. One `tokio::task` per display owns its framebuffer + `Notify`; coalescing prevents queue growth.

**Platform abstraction**: one `platform::DeviceClaim` trait per OS, but only the macOS backend is implemented in v0.1.
- macOS (v0.1 — **implemented**): `launchctl bootout gui/$UID/com.native-instruments.NIHostIntegrationAgent` pre-open (SIGSTOP fallback via `libc::kill`); restore in `Drop`. Also stop `Maschine.app` / `Komplete Kontrol.app` if detected via `pgrep`.
- Linux (v0.2 — stub, returns `Error::UnsupportedPlatform`): will ship `99-maschine.rules` udev file (17cc:1600), `USBDEVFS_DISCONNECT` any stray kernel driver.
- Windows (v0.2 — stub): interface #5 requires WinUSB. Will detect default-driver state and surface `Error::WindowsWinUsbRequired` with a one-time Zadig/libwdi instruction; interface #4 keeps working on HIDClass.

The trait exists from M1 so adding backends in v0.2 is purely additive — no refactor.

**Event model**: runtime-agnostic `futures::Stream<Item = Event>` backed by `tokio::sync::mpsc` (feature flag for smol). Per-pad hysteresis (`pad_on=0x4010`, `pad_off=0x4005`), 4ms minimum between `Pressure` events, ≥3 LSB delta filter. Attack velocity = peak over first 3–5 frames after crossing threshold. Raw-mode opt-in for users building custom curves. Bounded channel (1024) with latest-wins pad coalescing on overflow.

**NKS parser**: `riff` crate for chunk iteration, `rmp-serde` for MessagePack. Metadata-only scan (skip `PCHK` body, keep offset+len) for fast indexing; lazy `read_state()` at load time. Permissive `Option`-heavy structs — real NKS files omit fields.

**Index store**: SQLite + FTS5. Schema: `presets`, `preset_types`, `preset_modes`, `favorites`, `recents`, FTS5 mirror of `name+vendor+author+comment+bank_chain`. `notify` crate (FSEvents/ReadDirectoryChangesW/inotify) for incremental re-scan, 500ms debounce, mtime+size check before re-parse. `komplete.db3` read-only (rusqlite `immutable=1`) as an enrichment source, never authoritative.

**Plugin registry**: VST3 via `moduleinfo.json` fast path + subprocess scan fallback, keyed by Class UID `[u32;4]`. AU via `AudioComponentFindNext` from a C helper, keyed by `(type, subtype, manufacturer)`. PLID→registry match at index time, stored as stable string key (`vst3:aabbccdd:...`, `au:aumu:NIMX:-NI-`). Broken-plugin blocklist persists.

**Plugin session protocol** (`maschined` ↔ `maschine-pluginhost`):
- Control channel: UDS (macOS/Linux) or Named Pipe (Windows), line-delimited JSON.
- Audio: shared memory ring buffer, lock-free SPSC.
- MIDI in / parameter set / preset load via control channel.
- 5-second watchdog during plugin scan; segfault → mark blocked.

**Hardware UI** (v0.1 browse view): left display = faceted filter navigator (breadcrumb top, facet body, 4 quick-toggles bottom). Right display = preset list, ~8 rows, selected row emphasized. Encoder 1 scrolls facets, Encoder 2 drills down, Encoder 3 scrolls preset list, Encoder 4 adjusts preview volume. 16 pads double as top-level NKS type shortcuts (Synth / Bass / Lead / Pad / Keys / Drums / …). Transport buttons: Browse / Plugin / Mixer / Settings.

**External API**:
- OSC/UDP `127.0.0.1:57130`, OSC/WS `ws://127.0.0.1:57131`, JSON over UDS `$XDG_RUNTIME_DIR/maschined.sock`.
- Event addresses: `/mk3/pad/<i>/{down,up,pressure}`, `/mk3/button/<name>/{down,up}`, `/mk3/encoder/<i>/delta`, `/mk3/touchstrip/{pos,pressure}`.
- Command addresses: `/mk3/led/pad/<i> <rrggbb>`, `/mk3/led/button/<name> <value>`, `/mk3/display/<0|1>/image <url>`, `/mk3/browse/{filter,select,load}`.

### Testing strategy

- **Unit** (`maschine-proto`): every parser/encoder has byte-level round-trip tests. Display: hand-crafted framebuffers with one dirty tile asserted against exact output bytes.
- **Replay**: `maschine-capture` tool dumps usbmon/PacketLogger/USBPcap traces during scripted hardware sessions; tests feed IN frames through parser, diff OUT frames byte-for-byte.
- **Mock transport**: `MockTransport: Transport` backed by `VecDeque` drives the entire event loop without hardware. CI runs this on every push.
- **NKS corpus**: 100 curated `.nksf` files spanning Kontakt / Massive X / Battery / Reaktor / u-he / Plugin Alliance. Parser must round-trip all metadata without panic.
- **Hardware smoke** (`just hw-test`): local-only, flashes every LED, full pad pressure sweep, dumps new golden trace.

### Milestones (reference order)

1. **M1 — Device claim + LED blink.** `nusb` enumerate, stop NI agent, claim if#4+if#5, encode report `0x80/0x81`, cycle all LEDs + 16-pad rainbow. ~1 week.
2. **M2 — Input events.** Parse reports `0x01` and `0x02`, hysteresis + debounce, `examples/pad_monitor`, first replay golden tests. ~1–2 weeks.
3. **M3 — Display: full frame + dirty rects.** Header+chunk encoder, bulk transfer on `0x04`, both screens, `DirtyTracker`, 60fps `display_demo`. ~2 weeks.
4. **M4 — NKS parser + library index.** `nks-parse`, SQLite/FTS5 schema, `notify`-based rescan, 100-file corpus test. ~2 weeks.
5. **M5 — Plugin registry + pluginhost subprocess.** VST3 scan via `moduleinfo.json`, AU scan via AudioComponent, JUCE child process with JSON/shm IPC, load PCHK into live instance, play audible audio. ~3 weeks.
6. **M6 — Hardware UI (browse + load).** Left/right display model, encoder navigation, pad-shortcut filters, `/mk3/browse/load` end-to-end: twist encoders → load preset → hit pad → sound. ~2 weeks.
7. **M7 — External API + polish.** OSC/UDP, OSC/WS, UDS CLI (`msctl`), `embedded-graphics::DrawTarget` impl, notarized macOS bundle, launchd plist, docs.rs. ~1.5 weeks (down from 2 — no Windows MSI / systemd unit in v0.1).

**Total to v0.1: ~12 weeks solo** (macOS-only). v0.2 adds Linux (+1 week), Windows (+2 weeks incl. Zadig/libwdi installer), and native CoreAudio output replacing the loopback audio path.

### Critical files to create

- `/Users/tonic/Nerd/maschine/Cargo.toml` — workspace manifest
- `/Users/tonic/Nerd/maschine/crates/maschine-proto/src/{lib,hid_in,hid_out,display,types}.rs`
- `/Users/tonic/Nerd/maschine/crates/maschine-core/src/{transport,device,event}.rs`
- `/Users/tonic/Nerd/maschine/crates/maschine-core/src/display/{framebuffer,dirty,encoder}.rs`
- `/Users/tonic/Nerd/maschine/crates/maschine-core/src/platform/mod.rs` — trait definition
- `/Users/tonic/Nerd/maschine/crates/maschine-core/src/platform/macos.rs` — v0.1 implementation
- `/Users/tonic/Nerd/maschine/crates/maschine-core/src/platform/{linux,windows}.rs` — v0.2 stubs returning `Error::UnsupportedPlatform`
- `/Users/tonic/Nerd/maschine/crates/nks-parse/src/{lib,riff,nisi,plid,nica}.rs`
- `/Users/tonic/Nerd/maschine/crates/nks-index/src/{scanner,schema.sql,query,watch}.rs`
- `/Users/tonic/Nerd/maschine/crates/plugin-registry/src/{vst3,au,resolve}.rs`
- `/Users/tonic/Nerd/maschine/crates/maschine-ui/src/{browse,layout}.rs`
- `/Users/tonic/Nerd/maschine/crates/maschined/src/{main,osc,ws,session,ipc}.rs`
- `/Users/tonic/Nerd/maschine/pluginhost/{CMakeLists.txt, src/main.cpp, src/ipc.cpp, src/session.cpp}`
- `/Users/tonic/Nerd/maschine/msctl/src/main.rs`
- `/Users/tonic/Nerd/maschine/resources/macos/launchd.plist`
- `/Users/tonic/Nerd/maschine/examples/{blink,pad_monitor,display_demo,embedded_graphics}.rs`

### Reference implementations & specs to mine (external; read-only)

- **Display protocol** — `Drachenkaetzchen/cabl` fork, `doc/hardware/maschine-mk3/MaschineMK3-Display.md`. Byte layout of 32-byte header, `0x00/0x01/0x03/0x40` command chunks, 480×272 RGB565 big-endian framebuffer.
- **HID I/O** — same fork's `MaschineMK3-HIDInput.md` and `MaschineMK3-HIDOutput.md`. Report IDs `0x01/0x02` (IN), `0x80/0x81` (OUT). Pad pressure range 0x4000–0x4FFD.
- **USB descriptors** — `shaduzlabs/cabl#9`. 7 interfaces, if#4 HID, if#5 vendor bulk on EP 0x04, if#6 DFU (leave untouched).
- **Color palette + pad handling** — `asutherland/ni-controllers-lib` `lib/maschine_mk3.ts` + `lib/maschine_mk3_config.json`. Pad palette, encoder IDs, touch-strip event shape.
- **Live display RGB pipeline** — `asutherland/taskolio` (still active), `dkzeb/mixxx-mk3/screen-daemon`, `dkzeb/maschinepiJS` (PixiJS→framebuffer).
- **Python HID reference** — `Emerah/MMK3-HID-Control` + `Emerah/Prototype-NI-MMK3-HID` (pyusb display pixel push on macOS after NI agent suspend).
- **NKS format** — `jhorology/gulp-nks-rewrite-meta/nksf-file-format.txt` (community spec), `jhorology/nks-presets-collection` (examples), NI's public NKS-for-Developers page (plugin-side only, useful for NICA/controller-assignment layout).
- **NIHIA** — `terminar/rebellion` `scripts/niproto.lua` + `src/librebellion/platform/{macos,windows}/niproto_pimpl.cpp`. Not used in v0.1 but kept as a fallback research target for preset-browsing RE in v0.2.

### Verification

End-to-end, on real hardware:

1. `cargo run --example blink` — every LED and pad cycles through all colors; confirms if#4 write path, driver claim, NI-agent-stop.
2. `cargo run --example pad_monitor` — every pad press shows `Down(pressure, velocity)`, `Pressure(…)` at ≥4ms intervals, `Up`; every encoder step shows `Delta(±1)`; touch strip streams position+pressure. Confirms if#4 read + parsing.
3. `cargo run --example display_demo` — animated dirty rectangles at 60fps on both screens, no tearing, CPU < 5%. Confirms if#5 bulk write + framebuffer pipeline.
4. `cargo test -p maschine-proto` — all golden-byte and round-trip tests pass on CI with zero hardware.
5. `cargo test -p nks-parse` — all 100 files in the corpus parse, no panics on missing fields.
6. `maschined --scan-libraries && maschined status` — reports N presets indexed, M plugins resolved, K unresolved; rescan completes in <60s cold, <5s warm.
7. `msctl browse --filter 'type=Lead' --vendor 'Native Instruments'` — returns list; `msctl browse load <preset-id>` spawns pluginhost, loads Massive X with the preset state, injects MIDI note 60, audio appears at the configured output.
8. Hardware end-to-end: plug device in, launch `maschined`, press Browse button → left display shows facets, right shows preset list; twist encoders → navigation; press pad 1 while in browse mode → type-shortcut filter applies; hit load → audible sound on pad press.
9. `oscdump 57130` from a separate terminal — every hardware event emits the expected OSC address with correct payload.
10. Kill `maschined` mid-session — no hung processes; `ps aux | grep pluginhost` is empty; device re-claimable on next start.
11. Rebuild `komplete.db3` while `maschined` runs — `notify` delta rescan picks up new presets within 1 second of the database write settling.
