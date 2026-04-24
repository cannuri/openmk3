# openmk3

Open-source, cross-platform host for the **Native Instruments Maschine Mk3**
production controller. Direct USB, full HID, dual 480×272 color displays,
NKS preset browsing, VST3/AU plugin hosting — no Native Instruments software
required to drive the hardware.

> [!WARNING]
> Pre-alpha. The code builds, tests pass, and the architecture is locked, but
> you cannot flash LEDs on the device yet from a stock macOS install — macOS
> requires a signed DriverKit system extension to claim the Mk3's vendor
> interfaces, and that entitlement is under review with Apple (typical lead
> time 1–3 weeks). You can build and test end-to-end in macOS developer
> mode today. Linux hardware validation is next.

![status](https://img.shields.io/badge/status-pre--alpha-orange)
![license](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue)
![rust](https://img.shields.io/badge/rust-1.75+-blue)

## Why this exists

Native Instruments ships the Maschine Mk3 as a closed system: 16 pressure-
sensitive RGB pads, 8 endless encoders, touch strip, and two 480×272 color
screens — most of which are unreachable without NI's proprietary
Maschine/Komplete Kontrol software. In class-compliant MIDI mode the device
is a limited pad grid; the displays, per-pad RGB, and rich feedback stay
dark.

`openmk3` is an open, reverse-engineered host that gives you every hardware
surface through a clean Rust API, an OSC/WebSocket surface, and — when you
want — a full preset browser backed by an NKS parser and a JUCE-based
plugin host.

Credit for the protocol work that made this possible goes to
[shaduzlabs/cabl](https://github.com/shaduzlabs/cabl),
[Drachenkaetzchen/cabl](https://github.com/Drachenkaetzchen/cabl) (Mk3
display spec),
[asutherland/ni-controllers-lib](https://github.com/asutherland/ni-controllers-lib),
[Emerah/MMK3-HID-Control](https://github.com/Emerah/MMK3-HID-Control),
[terminar/rebellion](https://github.com/terminar/rebellion) (NIHIA IPC
reference), and
[jhorology/gulp-nks-rewrite-meta](https://github.com/jhorology/gulp-nks-rewrite-meta)
(NKS file format). This project would not exist without them.

## What works today

| Layer | Status | Platform |
|---|---|---|
| HID input/output wire protocol + encoders | ✅ 27 tests, round-trip verified | all |
| 480×272 display command-stream encoder | ✅ dirty-rect + 16×16 tile coalescing | all |
| NKS `.nksf` parser (NISI/PLID/NICA/PCHK) | ✅ tested against real factory content | all |
| Library index (SQLite + FTS5) + filesystem watcher | ✅ | all |
| VST3/AU plugin registry + PLID resolution | ✅ | macOS |
| JUCE pluginhost subprocess | ✅ scaffolded | macOS / Linux (Windows v0.2) |
| Hardware UI model (6×8 font, browse UI) | ✅ | all |
| `maschined` daemon + OSC/UDP + UDS + `msctl` CLI | ✅ | macOS |
| **Driving real hardware** | ⏳ needs DriverKit extension on macOS | see below |

## Architecture

```
┌──────────────────────────────────────────────────────────────┐
│                         maschined (Rust)                     │
│  ┌─────────────┐  ┌──────────────┐  ┌──────────────────┐     │
│  │ Transport   │  │ Display      │  │ Event bus (OSC/  │     │
│  │ trait       │─▶│ framebuffers │─▶│ UDS broadcast)   │     │
│  │             │  │ + dirty rect │  │                  │     │
│  └──────┬──────┘  └──────────────┘  └──────────────────┘     │
│         │ (platform impl)                                    │
│         ▼                                                    │
│  ┌─────────────────────┬──────────────────────┐              │
│  │ DextTransport (mac) │ NusbTransport (Linux │              │
│  │                     │ / Windows v0.2)      │              │
│  └──────────┬──────────┴──────────────────────┘              │
│             │ IOUserClient           │ usbfs                 │
│  ┌──────────▼───────────────┐ ┌──────▼─────────┐             │
│  │ Maschine Mk3 DriverKit   │ │ Mk3 via kernel │             │
│  │ system extension (.dext) │ │ USB subsystem  │             │
│  └──────────────────────────┘ └────────────────┘             │
└──────────────────────────────────────────────────────────────┘
                ↕ IPC (JSON + shm)
       maschine-pluginhost (JUCE, C++) — one per plugin
```

On **macOS** we ride a local DriverKit system extension that claims the
Mk3's HID (interface #4) and vendor-defined bulk (interface #5) interfaces
and exposes them to userspace via `IOUserClient`. On **Linux** we use
`nusb`/`usbfs` directly; no kernel module required.

Full architecture: [`dext/docs/A1-architecture.md`](dext/docs/A1-architecture.md).

## Quick start

### macOS — developer mode (free, works today)

```sh
# One-time: enable DriverKit developer mode (user-level, no reboot).
systemextensionsctl developer on

# One-time: disable SIP (requires booting into recovery).
# Hold power button on startup → Options → Utilities → Terminal:
#   csrutil disable
# Reboot back into macOS.

# Build.
git clone https://github.com/cannuri/openmk3
cd openmk3
./dext/scripts/build-dev.sh

# Install + approve the extension.
open dext/build/Maschine.app
# System Settings → General → Login Items & Extensions → Extensions →
# Driver Extensions → (i) → toggle Maschine on → authenticate.

# Try it.
cargo run --release --example blink       -p maschine-core    # rainbow pads
cargo run --release --example pad_monitor  -p maschine-core    # print events
cargo run --release --example display_demo -p maschine-core    # animate screens
cargo run --release -p maschined                               # the full daemon
cargo run --release -p msctl -- status                         # CLI
```

### macOS — production install (requires Apple DriverKit entitlement)

Apple gates the DriverKit USB entitlement. The request form is at
<https://developer.apple.com/contact/request/system-extension/>. Once
approved:

```sh
export MASCHINE_TEAM_ID=XXXXXXXXXX
export MASCHINE_DEVID_APP="Developer ID Application: Your Name (XXXXXXXXXX)"
export MASCHINE_DEVID_INSTALLER="Developer ID Installer: Your Name (XXXXXXXXXX)"
export MASCHINE_APPLE_ID=your@apple.id
export MASCHINE_NOTARY_PASSWORD='app-specific-password'
./dext/scripts/build-dist.sh
# produces dext/build/Maschine-Mk3-Host-0.1.0.pkg (signed, notarized, stapled)
```

End-user install guide: [`dext/docs/INSTALL.md`](dext/docs/INSTALL.md).

### Linux

Coming in v0.2 — the `nusb` transport path is in-tree but the Linux
platform-claim stub still needs to land + real-hardware verification on
a Pi/x86.

### Windows

v0.2+. WinUSB driver swap required for interface #5; interface #4 works on
the default HIDClass driver.

## Repository layout

```
crates/
├── maschine-proto/     # pure HID + display wire-format codecs (no I/O)
├── maschine-core/      # USB transport, event stream, display pipeline
│   └── src/transport/  # mod.rs facade → dext_impl.rs (macOS) or
│                       # nusb_impl.rs (Linux/Windows)
├── nks-parse/          # .nksf RIFF + MessagePack parser
├── nks-index/          # SQLite + FTS5 library index + fs watcher
├── plugin-registry/    # VST3 (moduleinfo.json) + AU (auval) scan
├── maschine-ui/        # 6×8 bitmap font, browse state machine
└── maschined/          # daemon binary (OSC, WS, UDS, session mgr)

msctl/                  # CLI client for maschined over UDS

pluginhost/             # C++/JUCE child process (one per loaded plugin)

dext/                   # macOS DriverKit system extension
├── MaschineDext.xcodeproj/
├── MaschineHost/       # .app container with activation request
├── MaschineMk3Dext/    # the .dext — 1268 LOC C++/IIG
│   ├── MaschineMk3HidTransport.{iig,cpp}       # if#4: AsyncIO
│   ├── MaschineMk3DisplayTransport.{iig,cpp}   # if#5: AsyncIOBundled, 16-slot ring
│   ├── MaschineMk3UserClient.{iig,cpp}         # ExternalMethod dispatch
│   └── MaschineIPC.h   # wire structs — byte-for-byte match with dext_wire.rs
├── scripts/            # build-dev.sh, build-dist.sh, install/uninstall
└── docs/               # research + architecture + end-user install guide

vendor/nusb/            # vendored nusb 0.1.14 with a documented macOS patch
                        # (kept for provenance + future Linux/Windows use)
```

## Development

```sh
cargo test --workspace           # 30 tests across the Rust workspace
cargo build --workspace --release
./dext/scripts/build-dev.sh      # builds the dext via xcodebuild
```

Running against real hardware requires either developer mode (see above) or
a distribution build signed with an approved DriverKit entitlement.

## Roadmap

- **v0.1** — macOS hardware bringup via DriverKit extension (dev-mode)
- **v0.1.1** — Linux direct-USB validation on Raspberry Pi + x86
- **v0.1.2** — NKS corpus hardening (Kontakt, Battery, Reaktor, u-he, Plugin Alliance)
- **v0.2** — Distribution-ready signed installer (pending Apple entitlement)
- **v0.2** — Windows WinUSB setup + installer
- **v0.3** — Pad color calibration, LED brightness mapping, touch-strip LED
- **v0.3** — Native CoreAudio / WASAPI output replacing the loopback path

See `docs/plan.md` for the full design plan from the initial
[plan-mode session](docs/plan.md) and the dext-alpha team's research output
under `dext/docs/`.

## Contributing

Issues and PRs welcome — especially:

1. Hardware testers with different Mk3 firmware revisions to verify the
   protocol constants (`maschine-proto::types`).
2. NKS files from vendors we haven't tested yet so we can expand the
   `nks-parse` corpus.
3. Linux users willing to run `cargo test` + `blink` + `pad_monitor` +
   `display_demo` on their distro and file the results.

Code style is plain `cargo fmt`. Tests must pass on `cargo test --workspace`.
Protocol changes need matching byte-level tests in `maschine-proto/tests/`.

## Legal

This is an **interoperability / reverse-engineering project**. It is not
affiliated with, endorsed by, or associated with Native Instruments GmbH.
Users of this software are expected to already own the Maschine Mk3
hardware; `openmk3` only enables userspace I/O to devices already in the
user's possession. No Native Instruments software, firmware, trademarks,
or content is distributed, bundled, modified, or impersonated by this
project.

## License

Dual-licensed under **MIT** or **Apache-2.0**, at your option. See
[LICENSE-MIT](LICENSE-MIT) and [LICENSE-APACHE](LICENSE-APACHE) (you can
also find the full Apache-2.0 text at
<https://www.apache.org/licenses/LICENSE-2.0>).
