// Owns a single loaded plugin + its JUCE audio graph.
#pragma once

#include <memory>
#include <string>

#include <juce_audio_processors/juce_audio_processors.h>
#include <juce_audio_devices/juce_audio_devices.h>

namespace maschine::pluginhost {

/// One loaded plugin, fed MIDI from `maschined` and producing audio via the
/// default JUCE audio device.
class Session {
public:
    Session();
    ~Session();

    /// Scan a single plugin bundle, returning JSON metadata.
    static std::string scan_bundle(const std::string& bundle_path);

    /// Load the plugin at `bundle_path`, restoring state from base64-encoded
    /// PCHK bytes. Returns a JSON descriptor on success.
    std::string load(const std::string& bundle_path, const std::string& state_base64);

    /// Feed one MIDI event.
    void midi(int status, int data1, int data2);

    /// Set a plugin parameter by index.
    void set_parameter(int index, float value);

    /// Stop audio + drop the plugin.
    void shutdown();

private:
    juce::AudioPluginFormatManager format_manager_;
    juce::AudioDeviceManager       device_manager_;
    std::unique_ptr<juce::AudioPluginInstance> plugin_;
    juce::MidiBuffer               pending_midi_;
    juce::CriticalSection          midi_lock_;
};

} // namespace maschine::pluginhost
