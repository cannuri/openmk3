#include "ipc.h"

#include <cstdio>
#include <cstdint>
#include <iostream>
#include <sstream>
#include <string>

namespace maschine::pluginhost {

namespace {
// Minimal JSON envelope parser — enough for our three fields. We intentionally
// avoid a heavyweight JSON dep here because the envelope shape is fixed and
// the body is already JSON-encoded by the daemon.
bool find_field(const std::string& s, const std::string& key, std::size_t& out_start, std::size_t& out_end) {
    auto k = "\"" + key + "\"";
    auto pos = s.find(k);
    if (pos == std::string::npos) return false;
    auto colon = s.find(':', pos + k.size());
    if (colon == std::string::npos) return false;
    auto v = colon + 1;
    while (v < s.size() && std::isspace(static_cast<unsigned char>(s[v]))) ++v;
    out_start = v;
    if (s[v] == '"') {
        ++v;
        while (v < s.size() && s[v] != '"') { if (s[v] == '\\') ++v; ++v; }
        out_end = v + 1;
    } else if (s[v] == '{') {
        int depth = 1; ++v;
        while (v < s.size() && depth > 0) { if (s[v] == '{') ++depth; else if (s[v] == '}') --depth; ++v; }
        out_end = v;
    } else {
        while (v < s.size() && s[v] != ',' && s[v] != '}') ++v;
        out_end = v;
    }
    return true;
}

std::string slice(const std::string& s, std::size_t start, std::size_t end) { return s.substr(start, end - start); }
std::string unquote(const std::string& s) {
    if (s.size() >= 2 && s.front() == '"' && s.back() == '"') return s.substr(1, s.size() - 2);
    return s;
}
}  // namespace

IpcMessage read_message() {
    std::string line;
    if (!std::getline(std::cin, line)) return {0, "", ""};
    std::size_t a, b;
    std::uint64_t id = 0;
    if (find_field(line, "id", a, b)) {
        try { id = std::stoull(slice(line, a, b)); } catch (...) {}
    }
    std::string kind;
    if (find_field(line, "kind", a, b)) kind = unquote(slice(line, a, b));
    std::string body = "{}";
    if (find_field(line, "body", a, b)) body = slice(line, a, b);
    return {id, std::move(kind), std::move(body)};
}

void write_message(const IpcMessage& m) {
    std::ostringstream ss;
    ss << "{\"id\":" << m.id << ",\"kind\":\"" << m.kind << "\",\"body\":" << (m.body_json.empty() ? "{}" : m.body_json) << "}\n";
    std::cout << ss.str();
    std::cout.flush();
}

void send_event(const std::string& kind, const std::string& body_json) {
    write_message({0, kind, body_json});
}

} // namespace maschine::pluginhost
