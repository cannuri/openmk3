# maschine

Open, cross-platform host for the Native Instruments Maschine Mk3 — direct
USB, full display support, NKS preset browsing, VST3/AU plugin hosting.
v0.1 targets macOS; Linux and Windows follow in v0.2.

## What works today (v0.1)

- `maschine-core` claims the Mk3 from macOS's `NIHostIntegrationAgent`, speaks
  the HID input/output protocol, and drives both 480×272 displays through the
  documented bulk-transfer command stream.
- `nks-parse` round-trips `.nksf` metadata (NISI / PLID / NICA / PCHK) and
  exposes a lazy `read_state()` for the plugin blob.
- `nks-index` walks filesystem roots, upserts into a SQLite+FTS5 index, and
  watches directories for live updates.
- `plugin-registry` enumerates installed VST3 (via `moduleinfo.json`) and AU
  (`auval -a`) plugins and resolves NKS `PLID` chunks against them.
- `maschine-ui` renders a working browser across the two displays with a
  simple 6×8 bitmap font; encoders 1 and 3 drive navigation.
- `maschined` stitches it all together, exposes OSC over UDP (`127.0.0.1:57130`)
  for events, and a UDS JSON control surface used by `msctl` (`status` /
  `browse` / `load`).
- `pluginhost` is a JUCE console app (built out-of-tree with CMake) that
  `maschined` spawns as a child process to load and play presets.

## Known issue on macOS (v0.1)

Modern macOS's `MIDIServer` opens every class-compliant USB MIDI device
*exclusively* at the IOUSB layer. The Mk3 advertises USB Audio Class on
interfaces 0–3, so `MIDIServer` grabs the whole device and our attempt to
claim interface #4 (HID) fails with `could not be opened for exclusive
access`.

`maschined` already kills `NIHostIntegrationAgent`, `NIHardwareAgent`,
`usbaudiod`, and `MIDIServer` before the claim, but `launchd` respawns
`MIDIServer` in under 50 ms, winning the race. Until the planned v0.1.1
fix (wrapping `USBInterfaceOpenSeize` from IOKit), the reliable workaround
is to unload the MIDIServer launch-agent for the duration of the session:

```sh
sudo launchctl unload /System/Library/LaunchAgents/com.apple.midiserver.plist
cargo run --release -p maschined
# …use the device…
sudo launchctl load   /System/Library/LaunchAgents/com.apple.midiserver.plist
```

If a crash leaves the NI background agents suspended (`STAT=T` in `ps`):

```sh
cargo run --example restore_agent -p maschine-core
```

## Quick start

```sh
# Build the Rust side.
cargo build --workspace --release

# Build the JUCE plugin host (optional for browse-only operation).
just pluginhost

# Run the daemon.
cargo run --release -p maschined
```

With the daemon running, in another terminal:

```sh
cargo run --release -p msctl -- status
# → {"ok":true,"payload":{"version":"0.1.0","device":"Maschine Mk3", ...}}

# Scan installed NKS libraries.
cargo run --release -p msctl -- browse --type Lead
# → {"ok":true,"payload":[{"id":...,"name":"...", ...}, ...]}

# Load a preset by id (browse first to get ids).
cargo run --release -p msctl -- load 42
```

Examples:

```sh
cargo run --release --example blink -p maschine-core
cargo run --release --example pad_monitor -p maschine-core
cargo run --release --example display_demo -p maschine-core
```

## OSC surface

Events (all on port 57130):

```
/mk3/pad/<0..15>/down         <pressure> <velocity>
/mk3/pad/<0..15>/pressure     <pressure>
/mk3/pad/<0..15>/up           <pressure>
/mk3/button/<bit>/{down,up}
/mk3/encoder/macro/<0..7>     <delta> <absolute>
/mk3/encoder/master           <delta> <absolute>
/mk3/touchstrip               <position> <pressure>
/mk3/touchstrip/release
/mk3/analog/<MicGain|Headphones|MasterVolume>   <value>
```

## Design

See [`study-all-those-projects-mighty-deer.md`](./docs/plan.md) for the full
architecture plan (copied from the plan mode approval file).

## License

MIT OR Apache-2.0. Protocol reverse-engineering credit goes to the
`shaduzlabs/cabl`, `Drachenkaetzchen/cabl`, `asutherland/ni-controllers-lib`,
`Emerah/MMK3-HID-Control`, `terminar/rebellion`, and
`jhorology/gulp-nks-rewrite-meta` projects — without their work this would not
be possible.
