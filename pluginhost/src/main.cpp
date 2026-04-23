// maschine-pluginhost: single-plugin child process driven by `maschined`
// through line-delimited JSON on stdio. See ipc.h for the protocol.

#include "ipc.h"
#include "session.h"

#include <cstdio>
#include <cstdlib>
#include <memory>
#include <string>

using namespace maschine::pluginhost;

namespace {
// Trivial JSON field extractor matching the subset we emit from Rust.
std::string read_string(const std::string& body, const std::string& key, const std::string& def = "") {
    auto k = "\"" + key + "\":\"";
    auto p = body.find(k);
    if (p == std::string::npos) return def;
    p += k.size();
    std::string out;
    while (p < body.size() && body[p] != '"') {
        if (body[p] == '\\' && p + 1 < body.size()) { out += body[p + 1]; p += 2; }
        else { out += body[p++]; }
    }
    return out;
}

int read_int(const std::string& body, const std::string& key, int def = 0) {
    auto k = "\"" + key + "\":";
    auto p = body.find(k);
    if (p == std::string::npos) return def;
    p += k.size();
    while (p < body.size() && std::isspace(static_cast<unsigned char>(body[p]))) ++p;
    try { return std::stoi(body.substr(p)); } catch (...) { return def; }
}

float read_float(const std::string& body, const std::string& key, float def = 0.0f) {
    auto k = "\"" + key + "\":";
    auto p = body.find(k);
    if (p == std::string::npos) return def;
    p += k.size();
    try { return std::stof(body.substr(p)); } catch (...) { return def; }
}
}  // namespace

int main(int argc, char** argv) {
    // Scan mode: short-lived sandbox for a single plugin introspection.
    if (argc == 3 && std::string(argv[1]) == "--scan") {
        auto json = Session::scan_bundle(argv[2]);
        write_message({0, "scan_result", json});
        return 0;
    }

    std::unique_ptr<Session> session = std::make_unique<Session>();
    while (true) {
        auto msg = read_message();
        if (msg.kind.empty()) break;
        if (msg.kind == "shutdown") {
            session->shutdown();
            write_message({msg.id, "ok", "{}"});
            break;
        } else if (msg.kind == "scan") {
            auto bundle = read_string(msg.body_json, "bundle");
            write_message({msg.id, "scan_result", Session::scan_bundle(bundle)});
        } else if (msg.kind == "load") {
            auto bundle = read_string(msg.body_json, "bundle");
            auto state = read_string(msg.body_json, "state_base64");
            write_message({msg.id, "load_result", session->load(bundle, state)});
        } else if (msg.kind == "midi") {
            int s = read_int(msg.body_json, "status");
            int d1 = read_int(msg.body_json, "note");
            int d2 = read_int(msg.body_json, "vel");
            session->midi(s, d1, d2);
            write_message({msg.id, "ok", "{}"});
        } else if (msg.kind == "set_param") {
            int i = read_int(msg.body_json, "index");
            float v = read_float(msg.body_json, "value");
            session->set_parameter(i, v);
            write_message({msg.id, "ok", "{}"});
        } else {
            write_message({msg.id, "error", "{\"error\":\"unknown verb\"}"});
        }
    }
    return 0;
}
