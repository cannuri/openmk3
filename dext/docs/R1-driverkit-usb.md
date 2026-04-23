# R1: DriverKit USB Deep-Dive for the Maschine Mk3 Dext

Researcher: `driverkit-researcher` · Project: `dext-alpha` · Target device: NI
Maschine Mk3 (VID `0x17cc`, PID `0x1600`) · Target host: Apple Silicon,
macOS 15 Sequoia minimum.

This brief answers the six questions in task #1. Every claim is cross-checked
against (a) the local DriverKit SDK headers shipped with Xcode 16.x
(`DriverKit 24.2`), (b) Apple's WWDC sample code, or (c) an Apple Developer
Forums response that was written by a DTS engineer. Where a source refused to
render (Apple's JS-only doc site), the item is backed by the SDK header
instead and marked as **SDK-verified**.

---

## 1. Which DriverKit framework version do we target?

The DriverKit xcframework is versioned independently of macOS. Xcode 16.3
ships `DriverKit.platform` as `DriverKit 24.2` with a supported range of
`MinimumDeploymentTarget = 19.0`, `RecommendedDeploymentTarget = 20.0`,
`MaximumDeploymentTarget = 24.2.99` (from
`/Applications/Xcode.app/Contents/Developer/Platforms/DriverKit.platform/Developer/SDKs/DriverKit.sdk/SDKSettings.json`).

Community/forum-inferred mapping (no official Apple table exists, per
developer.apple.com/forums/thread/719920):

| Host macOS | DriverKit deployment target |
|---|---|
| macOS 10.15 Catalina | 19.0 (initial) |
| macOS 11 Big Sur     | 20.0 |
| macOS 12 Monterey    | 21.0 / 21.4 |
| macOS 13 Ventura     | 22.x |
| macOS 14 Sonoma      | 23.x |
| macOS 15 Sequoia     | 24.0 / 24.1 / **24.2** |

**Recommendation:** set `DRIVERKIT_DEPLOYMENT_TARGET = 23.0` (Sonoma) so the
dext runs on 14 as well as 15. Every class and method we need is
`introduced=19.0` (see `IOUserUSBHostHIDDevice.iig` lines 59, 72, 102,
124, 137, 153, 173, 186). The one exception we hit later — HID `setReport`
over a dedicated interrupt-OUT pipe — is `introduced=20.0`. Nothing we need
is 23+/24+, so Sonoma as the floor gives us one extra macOS generation of
headroom without cost. Build artefact is produced by Xcode's DriverKit SDK
24.2; it is forward-compatible on Sequoia and Tahoe (per Apple's usual
DriverKit promise).

Frameworks we link against, in `DriverKit.platform/.../System/Library/Frameworks`:

* `DriverKit.framework` — core (`IOService`, `IOUserClient`, `OSAction`,
  `IOBufferMemoryDescriptor`, `IOTimerDispatchSource`, dispatch sources).
* `USBDriverKit.framework` — USB transport classes.
* `HIDDriverKit.framework` — HID transport classes (only needed if we
  subclass `IOUserUSBHostHIDDevice`; see §2 architecture choice).

---

## 2. Which classes do we subclass to claim if#4 HID and if#5 vendor bulk?

The Mk3 exposes seven USB interfaces. The two we need are:

* **if#4** — `bInterfaceClass = 0x03` (HID), pads/encoders/buttons/LEDs.
  This is the one `nusb`/IOKit currently loses with `kIOReturnExclusiveAccess`
  because Apple's in-box `AppleUserHIDDevice` dext has matched and
  opened the interface. A dext of ours, with a higher match score, replaces it.
* **if#5** — `bInterfaceClass = 0xFF` (vendor-specific), two bulk endpoints
  that carry pixel data for the dual 480×272 displays.

**Core provider class for both interfaces: `IOUSBHostInterface`** (SDK-verified
in `USBDriverKit.framework/Headers/IOUSBHostInterface.iig` line 53,
`class KERNEL IOUSBHostInterface : public IOService`). The dext driver that
targets a specific USB interface is matched against an `IOUSBHostInterface`
provider by the IOKit matcher.

We have two architecture choices for if#4:

### Option A — subclass `IOUSBHostInterface` directly for both interfaces

Two dexts (or two `IOKitPersonalities` in one dext bundle):

* Personality 1 (if#4): `IOProviderClass = IOUSBHostInterface`, match keys
  `idVendor=0x17cc`, `idProduct=0x1600`, `bInterfaceNumber=0x04`.
  `IOUserClass = MaschineHIDInterface` (our own subclass of `IOService`).
* Personality 2 (if#5): same match keys, `bInterfaceNumber=0x05`.
  `IOUserClass = MaschineBulkInterface`.

This is the pattern in Scott Knight's `USBApp` WWDC19 sample. Actual matching
dict from `MyUserUSBInterfaceDriver/Info.plist` (fetched from
github.com/knightsc/USBApp master):

```xml
<key>IOProviderClass</key>   <string>IOUSBHostInterface</string>
<key>IOClass</key>           <string>IOUserService</string>
<key>IOUserClass</key>       <string>MyUserUSBInterfaceDriver</string>
<key>IOUserServerName</key>  <string>sc.knight.MyUserUSBInterfaceDriver</string>
<key>bConfigurationValue</key> <integer>0x1</integer>
<key>bInterfaceNumber</key>    <integer>0x0</integer>
<key>idVendor</key>            <integer>0x781</integer>
<key>idProduct</key>           <integer>0x5530</integer>
```

And the `Start` body (same file, `MyUserUSBInterfaceDriver.cpp:67-103`),
which is the minimal scaffolding we need to claim the interface, copy a
pipe, allocate a DMA-safe buffer and enqueue an async read:

```cpp
ret = Start(provider, SUPERDISPATCH);
ivars->interface = OSDynamicCast(IOUSBHostInterface, provider);
ret = ivars->interface->Open(this, 0, NULL);
ret = ivars->interface->CopyPipe(kMyEndpointAddress, &ivars->inPipe);
ret = ivars->interface->CreateIOBuffer(kIOMemoryDirectionIn, maxPacketSize, &ivars->inData);
ret = OSAction::Create(this, MyUserUSBInterfaceDriver_ReadComplete_ID,
                       IOUSBHostPipe_CompleteAsyncIO_ID, 0, &ivars->ioCompleteCallback);
ret = ivars->inPipe->AsyncIO(ivars->inData, maxPacketSize, ivars->ioCompleteCallback, 0);
```

The ivars shape is also instructive (`MyUserUSBInterfaceDriver.cpp:41-48`):

```cpp
struct MyUserUSBInterfaceDriver_IVars {
    IOUSBHostInterface       *interface;
    IOUSBHostPipe            *inPipe;
    OSAction                 *ioCompleteCallback;
    IOBufferMemoryDescriptor *inData;
    uint16_t                  maxPacketSize;
};
```

With this option, *we* implement the HID report parsing and *we* expose
whatever userspace API we want. The OS doesn't see `IOHIDDevice` nodes at
all — which is exactly what we want for a DAW controller where the kernel
HID stack is what caused the conflict in the first place.

### Option B — subclass `IOUserUSBHostHIDDevice` for if#4

`HIDDriverKit.framework/Headers/IOUserUSBHostHIDDevice.iig:47`:

```cpp
class IOUserUSBHostHIDDevice : public IOUserHIDDevice
```

Provider is still `IOUSBHostInterface`. The framework does the pipe setup
for us and publishes an `IOHIDDevice` node (the same `IOHIDDevice` that
IOKit/`hidapi-rs` can open). Required overrides (same file, lines 102, 124,
137, 153, 173):

```cpp
virtual bool            handleStart(IOService *provider) override;
virtual OSDictionary*   newDeviceDescription() override;
virtual OSData*         newReportDescriptor() override;
virtual kern_return_t   getReport(IOMemoryDescriptor*, IOHIDReportType, IOOptionBits,
                                  uint32_t, OSAction*) override;
virtual kern_return_t   setReport(IOMemoryDescriptor*, IOHIDReportType, IOOptionBits,
                                  uint32_t, OSAction*) override;
```

`IOUserHIDDevice.iig:51-89` documents the `handleStart` / `newDeviceDescription` /
`newReportDescriptor` pattern — these are the pure virtual contract, not
`Start`. Critically (pqrs-org Karabiner-DriverKit-VirtualHIDDevice's
`DEVELOPMENT.md`): **do not override `Start` and do not call
`RegisterService` yourself** — set `kOSBooleanTrue` for key `RegisterService`
in the dictionary returned from `newDeviceDescription()` and the framework
will call `registerService` on your behalf. Karabiner's comment: "Calling
RegisterService in handleStart won't work because the initialization process
has not been completed." This is the #1 gotcha with `IOUserHIDDevice`.

### Ranked recommendation

1. **Option A (recommended, higher score).** Our userspace already speaks
   Maschine's private HID report format; we don't benefit from the kernel
   HID stack. Going through `IOHIDDevice` introduces exactly the surface
   that put us in `kIOReturnExclusiveAccess` in the first place (other
   processes' `IOHIDManager` will try to open our device, Karabiner-style
   "borrowing" can silently steal it, etc.). Direct `IOUSBHostInterface`
   gives us a private pipe to our own userspace client and nothing else. It
   is also simpler — no `IOHIDDevice` publishing, no report descriptor
   synthesis. This is the pattern used by `knightsc/USBApp` and by
   DanBurkhardt/DriverKitUserClientSample and is the shortest-path answer.
2. **Option B, deferred.** Only pick this if we decide we want third-party
   apps to also see the Mk3 through `IOHIDManager`. That is almost
   certainly a non-goal for v0.1.

If#5 (displays) has no HID stack to route through, so it is Option A
unconditionally.

---

## 3. How do we send/receive interrupt IN, interrupt OUT, and bulk OUT?

All three transfer types share one API on `IOUSBHostPipe`. SDK-verified in
`USBDriverKit.framework/Headers/IOUSBHostPipe.iig`:

```cpp
virtual kern_return_t  IO (IOMemoryDescriptor *dataBuffer,           // line 232
                           uint32_t            dataBufferLength,
                           uint32_t           *bytesTransferred,
                           uint32_t            completionTimeoutMs);

virtual kern_return_t  AsyncIO (IOMemoryDescriptor *dataBuffer,      // line 219
                                uint32_t            dataBufferLength,
                                OSAction           *completion TYPE(CompleteAsyncIO),
                                uint32_t            completionTimeoutMs);

virtual void           CompleteAsyncIO (OSAction *action TARGET,     // line 206
                                        IOReturn  status,
                                        uint32_t  actualByteCount,
                                        uint64_t  completionTimestamp) = 0;

virtual kern_return_t  Abort      (IOOptionBits, kern_return_t, IOService*);  // line 138
virtual kern_return_t  ClearStall (bool withRequest);                          // line 194
```

The direction of the transfer comes from the endpoint descriptor, not the
call — so the same `AsyncIO()` method handles interrupt IN, interrupt OUT,
and bulk OUT. The buffer's `kIOMemoryDirectionIn` / `kIOMemoryDirectionOut`
flag, combined with the endpoint's direction bit in `bEndpointAddress`,
tells the controller what to do. Important constraint the header calls out:
**`completionTimeoutMs` must be 0 for interrupt endpoints** (line 216,
"Must be 0 for interrupt endpoints").

Endpoints are obtained via `IOUSBHostInterface::CopyPipe(uint8_t address,
IOUSBHostPipe **)` (`IOUSBHostInterface.iig:136`) where `address` is the
full `bEndpointAddress` byte (direction bit included). For the Mk3, from
our prior `transport.rs` probe, those are `0x84` (HID interrupt IN on if#4),
`0x04` (HID interrupt OUT on if#4), and `0x05` / `0x06` (bulk OUT on if#5 for
the two displays — to be confirmed from the actual descriptor at bringup).

Buffers must be `IOBufferMemoryDescriptor` allocated via
`IOUSBHostInterface::CreateIOBuffer(IOOptionBits options, uint64_t capacity,
IOBufferMemoryDescriptor** buffer)` (`IOUSBHostInterface.iig:193`). The docstring
explicitly says "A buffer allocated by this method will not be bounced to
perform DMA operations" — that matters for the display path where we push
~522 kB per frame.

Async-IO flow in the dext (canonical pattern, assembled from `USBApp` +
`IOUSBHostPipe.iig`):

```cpp
// One-time setup in Start():
OSAction::Create(this, MyDriver_OnInRead_ID,
                 IOUSBHostPipe_CompleteAsyncIO_ID, 0, &ivars->inAction);

// Each read:
ivars->inPipe->AsyncIO(ivars->inBuf, 64, ivars->inAction, /*timeout=*/0);

// Completion in our IMPL method:
void IMPL(MaschineHIDInterface, OnInRead) {
    // status, actualByteCount, completionTimestamp are parameters
    if (status == kIOReturnSuccess) {
        ivars->userClient->DeliverReport(ivars->inBuf, actualByteCount);
        ivars->inPipe->AsyncIO(ivars->inBuf, 64, ivars->inAction, 0);   // requeue
    } else if (status == kIOUSBPipeStalled) {
        ivars->inPipe->ClearStall(true);
    }
}
```

There is also a throughput-focused bundled API for bulk (the displays):
`CreateMemoryDescriptorRing(size)` + `SetMemoryDescriptor(md, index)` +
`AsyncIOBundled(...)` with up to `kIOUSBHostPipeBundlingMax = 16` in flight
per call (`IOUSBHostPipe.iig:99, 275-335`). For 480×272 × 2 displays at
30 fps this is effectively required — a single in-flight transfer per frame
will starve. The completion shape is
`CompleteAsyncIOBundled(action, ioCompletionIndex, ioCompletionCount,
actualByteCountArray, ..., statusArray, ...)` (line 328). Docstring note:
"This method should only be used with bulk pipes" (line 295) — so use
`AsyncIO` for the interrupt IN on if#4 and `AsyncIOBundled` for the bulk
OUT on if#5.

For control-pipe requests (rarely needed by us, but useful for vendor
`wIndex`/`wValue` resets), use `IOUSBHostInterface::DeviceRequest` (sync) or
`AsyncDeviceRequest` (`IOUSBHostInterface.iig:219, 244`).

---

## 4. IPC between the dext and our Rust client app

**Recommended: `IOUserClient` with `ExternalMethod` dispatch + an async
completion OSAction for push notifications.** This is the Apple-blessed
path for DriverKit in 2025 and is used by every current sample. XPC is *not*
the DriverKit IPC story — a dext cannot vend an `NSXPCListener` from inside
its sandbox (no `NSXPCListener` class in DriverKit), and there is no
`NEAppProxyProvider`-style equivalent for USB. `IOServiceOpen` → user client
is the path.

The two-class pattern Apple endorses in the forum thread #710592 (fetched;
the DTS engineer's response paraphrases):

1. `MaschineHIDInterface : IOService` owns the pipes.
2. `MaschineUserClient   : IOUserClient` owns the mach port + dispatch
   table.
3. In `Info.plist`, the interface-driver personality has a
   `UserClientProperties` sub-dictionary:

```xml
<key>UserClientProperties</key>
<dict>
    <key>IOClass</key>      <string>IOUserUserClient</string>
    <key>IOUserClass</key>  <string>MaschineUserClient</string>
</dict>
```

4. Override `NewUserClient` on the interface driver to spawn the user client
   (from the same forum thread, verbatim):

```cpp
kern_return_t USBClass::NewUserClient_Impl(uint32_t type, IOUserClient** out) {
    IOService* client = nullptr;
    auto ret = Create(this, "UserClientProperties", &client);
    ivars->userClient = OSDynamicCast(MyUserClientClass, client);
    *out = ivars->userClient;
    return ret;
}
```

5. In the user client, declare a dispatch table. The canonical shape is from
   `DanBurkhardt/DriverKitUserClientSample` (fetched via WebFetch):

```cpp
static const IOUserClientMethodDispatch externalMethodChecks[kNumberOfMethods] = {
    [kMethod_SetLED] = {
        .function = (IOUserClientMethodFunction)&MaschineUserClient::Static_SetLED,
        .checkCompletionExists   = false,
        .checkScalarInputCount   = 0,
        .checkStructureInputSize = kVariableStructureSize, // use fixed for fixed payloads
        .checkScalarOutputCount  = 0,
        .checkStructureOutputSize = 0,
    },
    [kMethod_RegisterCallback] = {
        .function = ...,
        .checkCompletionExists   = true,   // <-- the async shape
        ...
    },
};
```

6. Async push (HID reports, display completion) is done by stashing the
   client-provided `OSAction` and calling back whenever a report comes in:

```cpp
// In RegisterCallback dispatch handler:
ivars->hidReportAction = arguments->completion;
ivars->hidReportAction->retain();

// Later, from the interrupt-IN completion:
AsyncCompletion(ivars->hidReportAction, kIOReturnSuccess,
                /* scalars */ nullptr, 0);
```

### Rust client side

Userspace uses the in-box `IOKit.framework`, *not* DriverKit. Calls:

```c
IOServiceGetMatchingServices(kIOMainPortDefault,
                             IOServiceNameMatching("MaschineUserClient"), &it);
IOServiceOpen(service, mach_task_self_, 0, &connect);      // kern_return_t
IOConnectCallStructMethod   (connect, kMethod_SetLED, inPtr, inLen, nullptr, nullptr);
IOConnectCallAsyncStructMethod(connect, kMethod_RegisterCallback,
                               /*wakePort*/ port, /*refs*/ asyncRef, 1,
                               in, inLen, out, &outLen);
```

In Rust we wrap these via `io-kit-sys`/`mach2` crates (both already usable
from `crates/maschine-core`). The async wake-port is dispatched from an
`IONotificationPort` running on a `CFRunLoop` thread; we then post decoded
events onto a Tokio mpsc for the rest of the app.

One CRITICAL subtlety: `IOConnectCallAsyncStructMethod` has a fixed-size
ABI for scalar/struct returns. For variable-size report bodies the idiomatic
pattern is: the dext writes the report into a shared
`IOMemoryDescriptor` that the client mapped via `IOConnectMapMemory64`, and
the async callback only carries `(seq, len)` scalars. For 64-byte HID input
reports at ~1 kHz this is actually fine without the shared memory trick — the
async scalar path has plenty of bandwidth. For display bulk completion we
won't need client-visible data at all; the client only needs to know "the
last frame landed" so it can submit the next one.

---

## 5. Apple sample code — URLs and 5-line excerpts

Confirmed-present Apple or canonical third-party samples:

1. **WWDC19 Session 702 — "System Extensions and DriverKit"** (session transcript
   at asciiwwdc.com/2019/sessions/702): defines the framework, establishes
   the .iig → C++ build pipeline, and walks through `USBDriverKit` with the
   interrupt-IN sample that lives in the companion repo.
   * Session: https://developer.apple.com/videos/play/wwdc2019/702/
   * Companion code: https://github.com/knightsc/USBApp (Scott Knight's
     mirror of the session sample).

2. **`knightsc/USBApp` — `MyUserUSBInterfaceDriver`** — IOUSBHostInterface
   match, Open, CopyPipe, AsyncIO. Already excerpted in §3. This is our
   skeleton.

3. **Apple DriverKit sample code index page** —
   https://developer.apple.com/documentation/driverkit/driverkit-sample-code
   (WebFetch returned only the page title — the page is JS-rendered, so
   the list has to be scraped from the archive; the most-referenced
   entries in forum answers are "Creating a driver using the DriverKit SDK"
   and the HID keyboard / stylus samples). Not load-bearing for us; the two
   samples below cover the same ground with actual source.

4. **`DanBurkhardt/DriverKitUserClientSample`** — the IOUserClient method
   dispatch table + async-callback registration pattern, reused verbatim
   in §4. Fetched; excerpts above.

5. **`pqrs-org/Karabiner-DriverKit-VirtualHIDDevice`** (particularly
   `DEVELOPMENT.md`) — the authoritative writeup of the
   `IOUserHIDDevice` subclassing contract and its sharp edges
   (`newDeviceDescription` instead of `RegisterService`, do-not-override-
   `Start`). Useful even though we are not picking Option B, because if
   we ever do expose an `IOHIDDevice` this is the minefield map.

6. **WWDC20 Session 10210 — "Modernize PCI and SCSI drivers with
   DriverKit"** — not directly applicable but confirms the dispatch-source
   and `OSAction` idioms are uniform across DriverKit transports.

There is **no Apple project called "ComBlock USB"**; repeated web searches
returned zero hits. Task description likely conflated it with
`USBApp` / `SimpleUSBDriver`. Marking as **unverified** and deferring — we
don't need it.

---

## 6. macOS version gotchas

* **Sequoia (15) moved the approval UI.** Approval for a system extension
  used to surface under *System Settings → Privacy & Security → Security*.
  On 15.0+ it is *System Settings → General → Login Items & Extensions*
  (jamf.com/blog/system-extension-changes-in-sequoia). First-run docs for
  end-users must be rewritten.
* **Sequoia lets users remove dexts.** In macOS 14 and earlier, an
  MDM-installed extension was hard to remove. In Sequoia, an admin user
  can disable or remove any system extension from the new Login Items &
  Extensions pane. If we ever ship via MDM we should set the new
  `NonRemovableSystemExtensions` / `NonRemovableFromUISystemExtensions`
  configuration-profile keys introduced in macOS 15 (same Jamf writeup).
* **`com.apple.developer.driverkit.transport.usb` is per-VID.** Not a
  simple boolean. The entitlement is an array of dicts keyed by `idVendor`
  (+ optional `idProduct`), and the *provisioning profile* must carry
  matching entries or the dext will not load with
  `Unsatisfied entitlements: com.apple.developer.driverkit.transport.usb`
  (developer.apple.com/forums/thread/798056). There is an Apple request
  form for this entitlement; timelines in the forum suggest 1–3 weeks.
  Example payload we need:

  ```xml
  <key>com.apple.developer.driverkit.transport.usb</key>
  <array>
    <dict>
      <key>idVendor</key>  <integer>6092</integer>  <!-- 0x17cc -->
    </dict>
  </array>
  ```

* **`com.apple.developer.driverkit`** (boolean) and
  **`com.apple.developer.system-extension.install`** (on the host app)
  are also required. The dext entitlements file in the WWDC sample only
  carries `com.apple.developer.driverkit` +
  `com.apple.developer.driverkit.transport.usb` (verified: fetched
  `MyUserUSBInterfaceDriver.entitlements`).
* **Development workflow:** SIP must be off and
  `systemextensionsctl developer on` required until the entitlement is
  granted (forum thread #127220). On Apple Silicon "Reduced Security"
  must be selected in Recovery. This blocks CI signing — plan around it
  early (see R2 task).
* **`AppleUserHIDDevice` incumbency.** On Sonoma+, Apple auto-matches a
  generic HID dext for anything with a HID descriptor. Our match score
  must beat it: include `idVendor` *and* `idProduct` *and*
  `bInterfaceNumber` *and* `bConfigurationValue` in the personality —
  each extra key raises the probe score. No `IOKitDebug` tricks needed
  if all four are present, but if matching still fails the fallback is
  adding `IOProbeScore` to the personality.
* **Ventura → Sonoma → Sequoia behavioural continuity.** The core
  `IOUSBHostPipe::AsyncIO` / `IOUSBHostInterface::CopyPipe` signatures
  have not changed since DriverKit 19.0. `CompleteAsyncIOBundled` has
  been stable since at least 20.0. No observed regressions affecting our
  endpoint shapes in the Sequoia release notes we searched. Safe.
* **Sequoia 15.4 / DriverKit 24.4 SCSI ABI break** — does not affect us
  (we don't link SCSIDriverKit) but flag it for any future plans to add
  a mass-storage hat.

---

## Recommended architecture (6 bullets)

1. **One dext bundle with two `IOKitPersonalities`, both matching
   `IOUSBHostInterface` with `idVendor=0x17cc idProduct=0x1600`.** Personality
   1 uses `bInterfaceNumber=0x04` (HID input), personality 2 uses
   `bInterfaceNumber=0x05` (display bulk). Both include `bConfigurationValue=1`
   to raise match score above Apple's generic HID dext.
2. **Do NOT subclass `IOUserUSBHostHIDDevice`.** Subclass plain
   `IOService` twice — `MaschineHIDInterface` and `MaschineDisplayInterface` —
   each keeping an `IOUSBHostInterface*` in ivars. Private transport only,
   no public `IOHIDDevice` node. Rationale: avoids the `kIOReturnExclusiveAccess`
   reappearing from some third-party `IOHIDManager` consumer, and removes
   the report-descriptor round-trip we don't need.
3. **All I/O through `IOUSBHostPipe::AsyncIO` (HID) and
   `AsyncIOBundled` with a 16-slot `IOMemoryDescriptorRing` (displays).**
   Buffers allocated via `IOUSBHostInterface::CreateIOBuffer` so DMA does
   not bounce. Interrupt pipes pass `completionTimeoutMs=0`. A single
   `OSAction` per direction, reused across completions.
4. **IPC: one `MaschineUserClient : IOUserClient` per interface
   driver, instantiated through `NewUserClient` + `UserClientProperties`.**
   Dispatch table covers `SetLED(struct)`, `SendDisplayFrame(struct)`,
   `GetStatus(scalar)`, and `RegisterHIDCallback(completion)`. HID
   reports are delivered back by invoking the stored completion
   `OSAction` from the interrupt-IN handler. Zero shared memory for v0.1.
5. **Rust client** uses `IOKit.framework` directly (not DriverKit) via
   `io-kit-sys` — match `MaschineUserClient` by name, `IOServiceOpen`,
   drive an `IONotificationPort` on a dedicated thread, fan events into
   the existing Tokio pipeline. This replaces the current `hidapi-rs`
   path in `crates/maschine-core/src/transport.rs`.
6. **Minimum target: DriverKit 23.0 (macOS 14 Sonoma).** Build with
   Xcode 16.3 / DriverKit 24.2 SDK. Entitlements: `com.apple.developer.driverkit`
   + `com.apple.developer.driverkit.transport.usb` with an `idVendor=0x17cc`
   dict. File the entitlement request with Apple *now* — lead time is the
   single biggest schedule risk in this whole project, since nothing past
   the Xcode "it builds" stage can be tested on Sequoia without it.

---

### Sources

* `DriverKit.platform/.../USBDriverKit.framework/Headers/IOUSBHostInterface.iig` (local, SDK 24.2)
* `DriverKit.platform/.../USBDriverKit.framework/Headers/IOUSBHostPipe.iig` (local, SDK 24.2)
* `DriverKit.platform/.../HIDDriverKit.framework/Headers/IOUserUSBHostHIDDevice.iig` (local, SDK 24.2)
* `DriverKit.platform/.../HIDDriverKit.framework/Headers/IOUserHIDDevice.iig` (local, SDK 24.2)
* https://github.com/knightsc/USBApp — `MyUserUSBInterfaceDriver.{cpp,iig,Info.plist,entitlements}`
* https://github.com/DanBurkhardt/DriverKitUserClientSample — `ExternalMethod` dispatch table + async registration
* https://github.com/pqrs-org/Karabiner-DriverKit-VirtualHIDDevice/blob/main/DEVELOPMENT.md — `IOUserHIDDevice` subclassing rules
* https://developer.apple.com/forums/thread/710592 — two-class (`IOService` + `IOUserClient`) DriverKit IPC pattern
* https://developer.apple.com/forums/thread/127220 — `systemextensionsctl developer` + SIP requirements for bring-up
* https://developer.apple.com/forums/thread/798056 — `com.apple.developer.driverkit.transport.usb` provisioning-profile format and gotchas
* https://developer.apple.com/forums/thread/719920 — DriverKit version ↔ macOS mapping (unanswered; community-inferred table used above, marked as such)
* https://www.jamf.com/blog/system-extension-changes-in-sequoia/ — Sequoia approval-UI move and new MDM keys
* https://asciiwwdc.com/2019/sessions/702 — WWDC19 702 transcript (confirms `OSAction` completion pattern)
* https://developer.apple.com/videos/play/wwdc2020/10210/ — WWDC20 10210 (uniformity of dispatch sources across DriverKit)
