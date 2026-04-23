default:
    @just --list

# Build the full Rust workspace in release mode.
build:
    cargo build --workspace --release

# Run the test suite.
test:
    cargo test --workspace

# Launch the daemon (use RUST_LOG=debug for verbose logs).
run:
    cargo run --release -p maschined

# Run the LED-blink hardware example.
blink:
    cargo run --release --example blink -p maschine-core

# Run the input debugger.
monitor:
    cargo run --release --example pad_monitor -p maschine-core

# Run the display animation demo.
display-demo:
    cargo run --release --example display_demo -p maschine-core

# Scan installed NKS libraries and build the index.
scan:
    echo '{"op":"scan_libraries"}' | nc -U $(runtime-sock)

# Ask the daemon for its status.
status:
    cargo run --release -p msctl -- status

# Configure + build the JUCE plugin host (requires CMake ≥ 3.22).
pluginhost:
    cmake -S pluginhost -B pluginhost/build -DCMAKE_BUILD_TYPE=Release
    cmake --build pluginhost/build -j

# Install the launchd plist so maschined auto-starts on login.
install-launchd:
    cp resources/macos/launchd.plist ~/Library/LaunchAgents/com.cannuri.maschined.plist
    launchctl load -w ~/Library/LaunchAgents/com.cannuri.maschined.plist

# Uninstall the launchd plist.
uninstall-launchd:
    launchctl unload -w ~/Library/LaunchAgents/com.cannuri.maschined.plist
    rm -f ~/Library/LaunchAgents/com.cannuri.maschined.plist

runtime-sock:
    echo "${XDG_RUNTIME_DIR:-/tmp}/maschined.sock"
