// IPC layer: line-delimited JSON over stdin/stdout between `maschined` and a
// `maschine-pluginhost` child process. Audio travels separately through a
// shared-memory ring buffer (see `session.cpp`).
//
// Control message envelope:
//
//   {"id": <u64>, "kind": "<verb>", "body": {...}}
//
// Replies carry the same `id`. Event messages from host→daemon use id=0.
//
// Verbs (v0.1):
//   scan          — body: {"bundle": "/path/to/Plugin.vst3"} → reply includes
//                   plugin metadata (name, vendor, categories, parameters)
//   load          — body: {"bundle":"…", "state_base64":"…"} → creates a live
//                   plugin instance with its state restored
//   midi          — body: {"status":0x90,"note":60,"vel":100}
//   set_param     — body: {"index":u32,"value":f32}
//   shutdown      — no body
//
// Events (host→daemon):
//   audio_started — body: {"sample_rate":48000,"block":512}
//   level         — body: {"rms_l":f32,"rms_r":f32}
//   log           — body: {"level":"info","msg":"…"}

#pragma once

#include <string>

namespace maschine::pluginhost {

struct IpcMessage {
    std::uint64_t id;
    std::string   kind;
    std::string   body_json;  // already-encoded JSON for the body
};

/// Read one `\n`-terminated JSON message from stdin. Returns an empty kind on EOF.
IpcMessage read_message();

/// Write one JSON message to stdout and flush.
void write_message(const IpcMessage& msg);

/// Convenience for sending a host→daemon event (id=0).
void send_event(const std::string& kind, const std::string& body_json);

} // namespace maschine::pluginhost
