---
task: R3 — prior-art dexts for HID/USB devices
owner: prior-art-scout
team: dext-alpha
date: 2026-04-23
---

# R3 — Prior-art survey: DriverKit extensions for HID / vendor USB on macOS

## Goal

Identify open-source macOS DriverKit system extensions (`.dext`) that already
speak USB and expose endpoints to user space, so we can fork or closely
reference one when building the Maschine Mk3 dext (VID=0x17cc, PID=0x1600,
interfaces #4 HID and #5 vendor bulk).

## Summary ranking (fork-ability)

| Rank | Project | Why |
|------|---------|-----|
| 1 | **knightsc/USBApp** | Exact shape we need: `IOUSBHostInterface` provider + `IOUSBHostPipe` bulk read. Minimal, MIT, easy to copy and extend to two interfaces. |
| 2 | **black-dragon74/ROG-HID** | Complete HID-interface dext + `IOUserClient` exposing a cdev-style API to a userland helper. BSD-3. Exact template for our HID half. |
| 3 | **pqrs-org/Karabiner-DriverKit-VirtualHIDDevice** | Production-grade dext with full Makefile, signing, packaging, XPC client, launchd daemon. Virtual HID, not USB — reference the *scaffolding*, not the driver logic. |
| 4 | **DanBurkhardt/DriverKitUserClientSample** | Apple's "Communicating between a DriverKit extension and a client app" WWDC sample, tidied. Canonical reference for user-client scalar/struct/async callbacks. |
| 5 | **Drewbadour/HIDDriverKitAccelerationDisableSample** | Tiny, modern (2025) HID-interface dext that shows `IOUserHIDEventDriver` subclassing + report filtering. Reference, not a fork base. |
| 6 | **Alkenso/USBFilterDriverKit** | Commercial USB filter dext (Swift+C++). Only partial source, no license — reference only. |

Anti-recommendations:

- **hansfbaier/open-maschine** — Maschine Mk2 proof-of-concept from 2015, pure
  userspace `hidapi` Python scripts. Zero DriverKit content. Useful only for
  the Mk2 HID vocabulary (display packets, LED layout) if we later need
  protocol hints, but it is not a dext.
- **Opa-/x1-mk1-usb2midi** — Rust userland app using `rusb`/`libusb`. No dext.
  Good Rust USB reference, irrelevant as a skeleton.
- **ktemkin/usrs** — Pure-Rust USB host library, macOS via IOKit, last push
  2023-01. Client-side, not a dext. Could inform our Rust `DextTransport`
  (task #7) but not the dext itself.
- **mrmidi/ASFireWire** — DriverKit OHCI FireWire host controller. Wrong bus.
- **dariaomelkina/MacOS_drivers_instruction** — UCU semester project, 2022,
  documentation only, no license. Reference prose, not code.

---

## Detailed candidate write-ups

### 1. knightsc/USBApp  (BEST FORK CANDIDATE for the bulk half)

- **URL**: https://github.com/knightsc/USBApp
- **Language**: C++ (dext) + Swift (container app)
- **License**: MIT — compatible with our MIT/Apache-2.0 repo.
- **Last push**: 2024-01-30 (stable, low-churn — this is the nature of a
  "hello world" sample).
- **Stars**: 92
- **Matching**: `IOProviderClass = IOUSBHostInterface`, matched by
  `idVendor`/`idProduct`/`bConfigurationValue`/`bInterfaceNumber` in
  `Info.plist` (example uses a Sandisk Cruzer). Exactly the matching key
  set we need for interface #5.
- **I/O exposure to userland**: None built in — the dext just reads the
  bulk-IN pipe and logs. But the plumbing we need is all present:
  `IOUSBHostInterface::CopyPipe`, `IOUSBHostPipe::AsyncIO`, `OSAction`
  completion callbacks, `IOBufferMemoryDescriptor` setup.
- **Is it a skeleton we can fork?** YES. It is the single closest thing to
  "minimal DriverKit vendor-bulk dext." Copy `MyUserUSBInterfaceDriver.{cpp,h,iig}`
  and `Info.plist`, duplicate for the second interface, bolt on a user-client
  for the Rust IPC path (borrow from ROG-HID / Apple NullDriver sample).

### 2. black-dragon74/ROG-HID  (BEST FORK CANDIDATE for the HID half + userclient)

- **URL**: https://github.com/black-dragon74/ROG-HID
- **Language**: C++
- **License**: BSD-3-Clause — compatible.
- **Last push**: 2022-07-25 (macOS 12-era; HIDDriverKit API has been stable
  since, so staleness is low risk).
- **Stars**: 47
- **Matching**: `IOProviderClass = IOHIDInterface` with multiple personalities
  for different ASUS models. Matches by VendorID + ProductID. Higher-level
  HID match rather than raw `IOUSBHostInterface` — this is what we want for
  Mk3 interface #4 if we let the OS's HID stack enumerate the interface
  first.
- **I/O exposure to userland**: Has a full `ROGHIDUserClient` subclass of
  `IOUserClient` — external methods, scalar + struct calls, async callbacks.
  This is the cleanest minimal example of the user-client side we've seen.
- **Is it a skeleton we can fork?** YES for the HID + user-client scaffolding.
  Ignore the ROG-specific report parsing. Reuse: project layout, `.iig`
  interface file, user-client dispatch table, `codesign.sh`, Makefile.

### 3. pqrs-org/Karabiner-DriverKit-VirtualHIDDevice  (BEST REFERENCE for packaging/signing)

- **URL**: https://github.com/pqrs-org/Karabiner-DriverKit-VirtualHIDDevice
- **Language**: C++ (dext) + C++/Swift (daemon, manager, client)
- **License**: Unlicense (public domain) — compatible, no attribution needed.
- **Last push**: 2026-04-19 (actively maintained).
- **Stars**: 349
- **Matching**: Virtual device — it *creates* HID devices (keyboard, pointing)
  using `IOUserHIDDevice`. It does NOT match on a physical USB device, so its
  `IOProviderClass` is `IOUserResources`, not what we need.
- **I/O exposure to userland**: Full stack — `Daemon/` (launchd service),
  `Manager/` (Swift CLI/app for sysext activation), `virtual-hid-device-service-client`
  example, `SMAppServiceExample`. XPC-based transport between the service
  and clients.
- **Is it a skeleton we can fork?** PARTIAL. Do not fork the driver logic —
  it is virtual-device, not host-side. DO crib from it:
  - `Makefile` + `make-package.sh` + `scripts/` for packaging/notarization.
  - `entitlements.plist` layout.
  - `Daemon/` for the launchd helper model if we decide on a daemon.
  - `vendor/` pinning pattern for third-party deps.
  - XPC service client template in `examples/virtual-hid-device-service-client`.

### 4. DanBurkhardt/DriverKitUserClientSample  (canonical user-client reference)

- **URL**: https://github.com/DanBurkhardt/DriverKitUserClientSample
- **Language**: C++ (dext) + Swift (host app)
- **License**: None declared — **licensing risk**. Apple sample code is normally
  under Apple's MIT-like "Sample Code License", but this repo has no LICENSE
  file. Safe to **reference and learn from**, not to copy-paste without
  rewriting.
- **Last push**: 2024-12-13
- **Stars**: 35
- **Matching**: `NullDriver`, `IOUserResources` — not a USB match. Focus is
  purely on the user-client IPC surface: scalar method, struct method,
  async callback, secure vs insecure validation patterns.
- **I/O exposure to userland**: This IS the reference for IOUserClient IPC.
  `CppUserClient` shows the `IOServiceOpen` / `IOConnectCallMethod` /
  `IONotificationPortCreate` async callback pattern end-to-end.
- **Is it a skeleton we can fork?** REFERENCE ONLY (license + scope). Use
  it to understand the Rust-side IOKit calls we need in task #7.

### 5. Drewbadour/HIDDriverKitAccelerationDisableSample  (modern minimal HID dext)

- **URL**: https://github.com/Drewbadour/HIDDriverKitAccelerationDisableSample
- **Language**: C++
- **License**: "Other" (custom; needs review before reuse).
- **Last push**: 2025-05-12 (most recent of any HID dext sample).
- **Stars**: 0
- **Matching**: `IOHIDInterface` / `IOUserHIDEventDriver` — edits pointer
  packets mid-flight.
- **Is it a skeleton we can fork?** NO (license unclear, filter-style rather
  than claim-style). Useful as a modern reference for `IOUserHIDEventDriver`
  subclassing if we end up wanting the OS HID stack to see our device.

### 6. Alkenso/USBFilterDriverKit  (reference only; commercial)

- **URL**: https://github.com/Alkenso/USBFilterDriverKit
- **Language**: Swift (framework) + C++ (dext)
- **License**: None declared. Marked "Commercial USB Filter solution."
- **Last push**: 2021-10-18
- **Stars**: 6
- **Matching**: USB filter / enumerator dext.
- **Is it a skeleton we can fork?** NO — no license, commercial framing, old.
  Listed because the repo demonstrates a Swift-framework-over-dext topology
  which may inform the macOS side of our Rust bindings.

---

## Apple first-party samples (reference, not on GitHub directly)

Two official samples are relevant and downloadable from developer.apple.com
as Xcode projects:

- **"Handling USB device-to-host communication with a driver extension"**
  — `IOUSBHostInterface` provider + bulk-IN pipe. This is what knightsc/USBApp
  was derived from. Good to diff against the GitHub version when in doubt.
- **"Communicating between a DriverKit extension and a client app"**
  — The `NullDriver` + `CppUserClient` pair. What DanBurkhardt/DriverKitUserClientSample
  tracks. Canonical for our user-client design.

Both ship under Apple's Sample Code License, which is MIT-ish and compatible
with our project.

---

## Recommendation for task #4 (A1: architecture + IPC)

Compose the dext from three known-good reference slices:

1. **Interface matching + bulk IO** — port
   `knightsc/USBApp:MyUserUSBInterfaceDriver.*` into two sibling classes,
   one for interface #4 (HID) and one for interface #5 (bulk). Adjust the
   `Info.plist` personalities for VID=0x17cc PID=0x1600 and
   `bInterfaceNumber` 4 and 5.
2. **User-client dispatch** — copy the `IOUserClient` skeleton from
   `black-dragon74/ROG-HID:ROGHIDUserClient`. Keep the external-method
   dispatch table small (open, close, write, enable-async-reads, async
   callback).
3. **Packaging / signing / activation** — borrow Makefile targets,
   `entitlements.plist` shape, and SMAppService activation from
   `pqrs-org/Karabiner-DriverKit-VirtualHIDDevice`.

This gives us a dext whose every file has a known-working reference, and
keeps our own invention budget focused on the Mk3-specific pieces
(matching keys, pipe enumeration for the two interfaces, and the Rust
transport on the client side).

---

## Source links

- knightsc/USBApp — https://github.com/knightsc/USBApp
- black-dragon74/ROG-HID — https://github.com/black-dragon74/ROG-HID
- pqrs-org/Karabiner-DriverKit-VirtualHIDDevice — https://github.com/pqrs-org/Karabiner-DriverKit-VirtualHIDDevice
- DanBurkhardt/DriverKitUserClientSample — https://github.com/DanBurkhardt/DriverKitUserClientSample
- Drewbadour/HIDDriverKitAccelerationDisableSample — https://github.com/Drewbadour/HIDDriverKitAccelerationDisableSample
- Alkenso/USBFilterDriverKit — https://github.com/Alkenso/USBFilterDriverKit
- ktemkin/usrs — https://github.com/ktemkin/usrs
- hansfbaier/open-maschine — https://github.com/hansfbaier/open-maschine
- Opa-/x1-mk1-usb2midi — https://github.com/Opa-/x1-mk1-usb2midi
- Apple DriverKit sample code index — https://developer.apple.com/documentation/driverkit/driverkit-sample-code
- Apple "Communicating between a DriverKit extension and a client app" — https://developer.apple.com/documentation/DriverKit/communicating-between-a-driverkit-extension-and-a-client-app
