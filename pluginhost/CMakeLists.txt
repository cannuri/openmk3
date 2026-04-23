cmake_minimum_required(VERSION 3.22)
project(maschine_pluginhost LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

# JUCE is expected to be available either as a submodule under ./JUCE or via
# a locally installed copy. The FetchContent path keeps the repo shippable
# without committing a vendored JUCE tree.
include(FetchContent)
FetchContent_Declare(
    JUCE
    GIT_REPOSITORY https://github.com/juce-framework/JUCE.git
    GIT_TAG        7.0.12
)
FetchContent_MakeAvailable(JUCE)

juce_add_console_app(maschine-pluginhost
    PRODUCT_NAME "maschine-pluginhost"
)

target_sources(maschine-pluginhost PRIVATE
    src/main.cpp
    src/ipc.cpp
    src/session.cpp
)

target_compile_definitions(maschine-pluginhost PRIVATE
    JUCE_PLUGINHOST_VST3=1
    JUCE_PLUGINHOST_AU=1
    JUCE_WEB_BROWSER=0
    JUCE_USE_CURL=0
)

target_link_libraries(maschine-pluginhost PRIVATE
    juce::juce_audio_basics
    juce::juce_audio_devices
    juce::juce_audio_formats
    juce::juce_audio_processors
    juce::juce_core
    juce::juce_data_structures
    juce::juce_events
)
