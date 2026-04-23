#include "session.h"
#include "ipc.h"

#include <sstream>

namespace maschine::pluginhost {

namespace {
std::string json_escape(const std::string& s) {
    std::string out; out.reserve(s.size() + 2);
    for (char c : s) {
        switch (c) {
            case '"': out += "\\\""; break;
            case '\\': out += "\\\\"; break;
            case '\n': out += "\\n"; break;
            case '\r': out += "\\r"; break;
            case '\t': out += "\\t"; break;
            default:
                if (static_cast<unsigned char>(c) < 0x20) {
                    char buf[8]; std::snprintf(buf, sizeof(buf), "\\u%04x", c);
                    out += buf;
                } else out += c;
        }
    }
    return out;
}

juce::MemoryBlock decode_base64(const juce::String& b64) {
    juce::MemoryBlock mb;
    juce::MemoryOutputStream out(mb, false);
    juce::Base64::convertFromBase64(out, b64);
    return mb;
}
}  // namespace

Session::Session() {
    format_manager_.addDefaultFormats();
    device_manager_.initialiseWithDefaultDevices(/*numIn*/0, /*numOut*/2);
}

Session::~Session() { shutdown(); }

std::string Session::scan_bundle(const std::string& bundle_path) {
    juce::AudioPluginFormatManager fm;
    fm.addDefaultFormats();
    juce::OwnedArray<juce::PluginDescription> found;
    juce::KnownPluginList list;
    for (auto* fmt : fm.getFormats()) {
        list.scanAndAddDragAndDroppedFiles(fm, {juce::File(bundle_path)}, found);
    }
    if (found.isEmpty()) return "{\"ok\":false,\"error\":\"no plugin found in bundle\"}";
    const auto* d = found[0];
    std::ostringstream ss;
    ss << "{\"ok\":true,\"name\":\"" << json_escape(d->name.toStdString())
       << "\",\"vendor\":\"" << json_escape(d->manufacturerName.toStdString())
       << "\",\"format\":\"" << json_escape(d->pluginFormatName.toStdString())
       << "\",\"category\":\"" << json_escape(d->category.toStdString())
       << "\",\"numInputs\":" << d->numInputChannels
       << ",\"numOutputs\":" << d->numOutputChannels
       << ",\"isInstrument\":" << (d->isInstrument ? "true" : "false")
       << "}";
    return ss.str();
}

std::string Session::load(const std::string& bundle_path, const std::string& state_b64) {
    juce::OwnedArray<juce::PluginDescription> descs;
    juce::KnownPluginList list;
    list.scanAndAddDragAndDroppedFiles(format_manager_, {juce::File(bundle_path)}, descs);
    if (descs.isEmpty()) return "{\"ok\":false,\"error\":\"plugin not found\"}";
    juce::String error;
    auto instance = format_manager_.createPluginInstance(*descs[0], 48000.0, 512, error);
    if (!instance) return std::string("{\"ok\":false,\"error\":\"") + json_escape(error.toStdString()) + "\"}";

    if (!state_b64.empty()) {
        auto mb = decode_base64(juce::String(state_b64));
        instance->setStateInformation(mb.getData(), static_cast<int>(mb.getSize()));
    }

    plugin_ = std::move(instance);
    plugin_->prepareToPlay(48000.0, 512);

    send_event("audio_started", "{\"sample_rate\":48000,\"block\":512}");
    std::ostringstream ss;
    ss << "{\"ok\":true,\"numParams\":" << plugin_->getParameters().size() << "}";
    return ss.str();
}

void Session::midi(int status, int d1, int d2) {
    juce::ScopedLock sl(midi_lock_);
    pending_midi_.addEvent(juce::MidiMessage(status, d1, d2), 0);
}

void Session::set_parameter(int index, float value) {
    if (!plugin_) return;
    auto& ps = plugin_->getParameters();
    if (index >= 0 && index < ps.size()) {
        ps[index]->setValueNotifyingHost(value);
    }
}

void Session::shutdown() {
    if (plugin_) {
        plugin_->releaseResources();
        plugin_.reset();
    }
    device_manager_.removeAllChangeListeners();
}

} // namespace maschine::pluginhost
