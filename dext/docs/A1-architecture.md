# A1 — Architecture and IPC Protocol for the Maschine Mk3 DriverKit Extension

Owner: `architect` · Project: `dext-alpha` · Depends on: R1 (DriverKit USB),
R2 (Packaging), R3 (Prior-art). Blocks: A2 (Xcode scaffold), I1 (dext plumbing),
I2 (Rust DextTransport), P1 (signing + installer).

This document locks the concrete shape of the dext, the dext ↔ userspace wire
protocol, the Rust integration, and the build system. Decisions already made in
research (single dext with two personalities, plain `IOService` subclasses,
`AsyncIO` for HID, `AsyncIOBundled` for displays, `IOUserClient` with an
ExternalMethod dispatch table, DriverKit 23.0 floor, fork from knightsc/USBApp
augmented by DanBurkhardt/DriverKitUserClientSample) are taken as given.

---

## 1. Project layout

**Containment model.** The dext ships inside a **single `.app` container**
(`Maschine.app`). The container holds:

- `Contents/MacOS/maschined` — existing Rust daemon, unchanged binary name.
- `Contents/Library/SystemExtensions/com.cantonic.maschine.dext.dext` — the
  DriverKit bundle.
- `Contents/Resources/…` — existing resource assets.
- `Contents/embedded.provisionprofile` — host-app provisioning profile.

The host-app container is non-negotiable: `OSSystemExtensionRequest` (the only
supported activation API for a user-installed dext) requires a hosting
`.app` in `/Applications`, and the `userclient-access` entitlement must name a
bundle ID that is a prefix-descendant of the host app ID (R2 §1.4, §10 gotcha
#4). The `.app` has no user-visible UI today — it's a shell whose only purpose
is to (a) contain the dext, (b) hold the host-app entitlement, (c) launch
`maschined` when double-clicked. A GUI can grow into this shell later.

We do **not** split host and dext into separate distribution artefacts. That
path would require a separate host App ID with its own userclient-access
entitlement grant, extra notarisation cycles, and more complex install docs —
all cost with no gain for v0.1.

**Repo layout:**

```
maschine/
├─ Cargo.toml                     # workspace (unchanged)
├─ crates/
│  ├─ maschine-core/              # transport + event loop
│  │  └─ src/
│  │     ├─ transport.rs          # <--- becomes a thin façade (see §4)
│  │     ├─ transport/
│  │     │  ├─ nusb.rs            # existing nusb path, moved here
│  │     │  └─ dext.rs            # NEW: DextTransport, macOS release builds
│  │     └─ platform/             # (unchanged)
│  ├─ maschine-proto/             # wire constants (VID/PID/interfaces)
│  │  └─ src/
│  │     └─ dext_ipc.rs           # NEW: selector IDs + struct layouts shared with C++
│  ├─ maschined/                  # daemon (unchanged Rust src)
│  └─ …
├─ dext/
│  ├─ docs/
│  │  ├─ R1-driverkit-usb.md      # existing
│  │  ├─ R2-packaging.md          # existing
│  │  ├─ R3-prior-art.md          # existing
│  │  └─ A1-architecture.md       # <-- this file
│  ├─ Maschine.xcodeproj/         # NEW (from A2)
│  ├─ MaschineDext/               # NEW: dext target source
│  │  ├─ Info.plist               # two IOKitPersonalities (if#4, if#5)
│  │  ├─ MaschineDext.entitlements
│  │  ├─ MaschineMk3HidTransport.iig / .cpp   # HID interface driver (if#4)
│  │  ├─ MaschineMk3DisplayTransport.iig/.cpp # display bulk driver (if#5)
│  │  ├─ MaschineMk3UserClient.iig/.cpp       # ExternalMethod dispatch
│  │  └─ MaschineIPC.h            # shared selector IDs + structs (C/C++/Rust)
│  ├─ MaschineHost/               # NEW: minimal .app shell
│  │  ├─ Info.plist
│  │  ├─ MaschineHost.entitlements
│  │  └─ main.swift               # launches maschined, requests activation
│  ├─ build.sh                    # shell front-end invoking xcodebuild
│  └─ scripts/
│     ├─ sign.sh                  # the R2 codesign recipe
│     ├─ notarize.sh              # notarytool submit + staple
│     └─ pkgbuild.sh              # pkgbuild + productbuild
└─ pluginhost/                    # unchanged
```

**Every new file, one-line purpose:**

| Path | Purpose |
|---|---|
| `dext/MaschineDext/Info.plist` | Two `IOKitPersonalities` matching `IOUSBHostInterface` @ VID 0x17CC / PID 0x1600 on `bInterfaceNumber` 4 and 5, each spawning a `MaschineMk3UserClient` via `UserClientProperties`. |
| `dext/MaschineDext/MaschineDext.entitlements` | `driverkit`, `driverkit.transport.usb` (with `idVendor=6092`), `driverkit.family.usb.pipe`. |
| `dext/MaschineDext/MaschineMk3HidTransport.{iig,cpp}` | HID personality: claims if#4, copies pipes 0x84/0x03, stands up one `AsyncIO` read loop and a ring of OUT `OSAction`s. |
| `dext/MaschineDext/MaschineMk3DisplayTransport.{iig,cpp}` | Display personality: claims if#5, copies bulk-OUT pipes, stands up a 16-slot `IOMemoryDescriptorRing` and serves `AsyncIOBundled` submissions. |
| `dext/MaschineDext/MaschineMk3UserClient.{iig,cpp}` | `IOUserClient` subclass, ExternalMethod dispatch table, async completion stash. |
| `dext/MaschineDext/MaschineIPC.h` | Canonical C header with selector enums and POD structs; included by the dext and bindgen'd into `maschine-proto::dext_ipc`. |
| `dext/MaschineHost/Info.plist` | Host-app bundle identity, `LSBackgroundOnly=YES`, `NSSystemExtensionUsageDescription`. |
| `dext/MaschineHost/MaschineHost.entitlements` | `driverkit.userclient-access`, `system-extension.install`. |
| `dext/MaschineHost/main.swift` | Calls `OSSystemExtensionManager.activationRequest`, then execs the bundled `maschined`. |
| `dext/build.sh` | Wrapper: `xcodebuild -scheme MaschineDext -configuration <cfg>` + dext copy into `Contents/Library/SystemExtensions`. |
| `dext/scripts/{sign,notarize,pkgbuild}.sh` | Extracted verbatim from R2 §4–§5 so CI and humans share one source of truth. |
| `crates/maschine-core/src/transport/dext.rs` | `DextTransport` — opens the `MaschineMk3UserClient` via IOKit, exposes the same async API as the nusb transport. |
| `crates/maschine-core/src/transport/nusb.rs` | Existing nusb implementation, moved (no logic change). |
| `crates/maschine-proto/src/dext_ipc.rs` | Rust mirror of `MaschineIPC.h` (selector constants + `#[repr(C)]` POD structs). |

---

## 2. Class hierarchy inside the dext

Three classes, no more. All are DriverKit (IIG) types.

### 2.1 `MaschineMk3HidTransport : IOService`

Provider: `IOUSBHostInterface`, matched on if#4.

**State (ivars):**
```cpp
struct MaschineMk3HidTransport_IVars {
    IOUSBHostInterface      *interface;
    IOUSBHostPipe           *inPipe;         // 0x84, 64-byte interrupt IN
    IOUSBHostPipe           *outPipe;        // 0x03, 64-byte interrupt OUT
    IOBufferMemoryDescriptor *inBuf;         // single 64-byte DMA buffer
    OSAction                *inReadAction;   // completion for AsyncIO(inPipe)
    OSAction                *outWriteAction[kHidOutRingDepth];   // 8 slots
    IOBufferMemoryDescriptor *outBuf[kHidOutRingDepth];
    uint8_t                  outHead, outTail;
    MaschineMk3UserClient   *userClient;     // set by NewUserClient_Impl
    IOLock                  *outLock;
    uint32_t                 seqCounter;     // monotonic HID-IN sequence
};
```

**Lifecycle:**

- `Start(provider, SUPERDISPATCH)` → `OSDynamicCast<IOUSBHostInterface>` →
  `interface->Open(this, 0, NULL)` → `CopyPipe(0x84, &inPipe)` and
  `CopyPipe(0x03, &outPipe)` → allocate `inBuf` (64 B, in) and
  `outBuf[0..7]` (64 B, out) via `interface->CreateIOBuffer` →
  `OSAction::Create` for `inReadAction` and each `outWriteAction[i]` →
  kick the first read via `inPipe->AsyncIO(inBuf, 64, inReadAction, 0)` →
  `RegisterService()`.
- `NewUserClient_Impl(type, out)` → `Create(this, "UserClientProperties",
  &client)` → stash `userClient` → `*out = userClient`. (Canonical §1.4
  pattern from R1.)
- `Stop` / `free` → `Abort` both pipes, release OSActions, release IOBuffers,
  `interface->Close(this, 0)`.

**Interrupt-IN callback** (`IMPL(MaschineMk3HidTransport, OnHidReadComplete)`):
Called by the USB framework with `(status, actualByteCount, timestamp)`.
If `status == kIOReturnSuccess && actualByteCount > 0`, it calls
`userClient->DeliverHidReport(inBuf, actualByteCount, seqCounter++)`.
The user-client delivery is a single in-process method call that invokes
the stored async `OSAction` (see §3). If `status == kIOUSBPipeStalled`,
`ClearStall(true)` and fall through. In all cases, requeue the read:
`inPipe->AsyncIO(inBuf, 64, inReadAction, 0)`. There is exactly **one**
in-flight IN read at a time — HID at ~1 kHz is bandwidth-trivial and a
single outstanding transfer simplifies ordering.

**Interrupt-OUT queue.** Up to 8 outstanding writes. Each user-client
`HID_OUT_REPORT` call picks a slot from the free ring (`outHead`), copies
the payload into `outBuf[slot]`, submits `outPipe->AsyncIO(outBuf[slot],
len, outWriteAction[slot], 0)`, and advances `outHead`. The paired
completion `IMPL(OnHidWriteComplete)` receives the slot index as the
OSAction's action-context field and releases it back onto the tail
(`outTail`). If the ring is full, the user client synchronously returns
`kIOReturnNoResources` to the Rust side, which maps to backpressure.

### 2.2 `MaschineMk3DisplayTransport : IOService`

Provider: `IOUSBHostInterface`, matched on if#5.

**State (ivars):**
```cpp
struct MaschineMk3DisplayTransport_IVars {
    IOUSBHostInterface      *interface;
    IOUSBHostPipe           *bulkOut;        // EP 0x04 (both displays share it on Mk3; confirmed in descriptor probe)
    IOMemoryDescriptorRing  *ring;           // 16 slots, 520 kB each
    IOBufferMemoryDescriptor *slots[kDisplayRingDepth];
    OSAction                *bundledAction;  // single OSAction for AsyncIOBundled
    MaschineMk3UserClient   *userClient;
    uint8_t                  head, tail;
    IOLock                  *ringLock;
};
```

**Bulk-OUT scheduling.** Frames are **scheduled per-submission, not
per-frame**. The user client's `BULK_OUT` method copies the payload into
`slots[head]`, binds it via `ring->SetMemoryDescriptor(slots[head], head)`,
and calls `bulkOut->AsyncIOBundled(ring, /*startIndex=*/head,
/*count=*/1, bundledAction, 0)` with bundling-count 1. That leaves the
controller free to coalesce submissions into the 16-slot ring when
userspace submits several back-to-back, while preserving per-frame
completion semantics that the Rust display pipeline already expects. When
`head - tail == kDisplayRingDepth`, the call returns
`kIOReturnNoResources` synchronously.

`CompleteAsyncIOBundled` (our `IMPL(OnDisplayWriteComplete)`) receives
`(action, ioCompletionIndex, ioCompletionCount, actualBytes[], status[])`
and, for each completed slot, bumps `tail` and fires an async notification
back to userspace via `userClient->NotifyDisplayComplete(seq, status)`.

We deliberately do **not** run a single permanent 16-deep bundled
submission from `Start` (which some reference code does). Userspace is the
rate source; if nothing submits, nothing should be in flight.

### 2.3 `MaschineMk3UserClient : IOUserClient`

One instance per opened connection. Each USB interface driver creates its
own user client — so one Rust `DextTransport` holds **two** IOConnect
handles, one per personality. The user client knows which side it is via
an ivar set at `Start` time (from the provider's class).

**State (ivars):**
```cpp
struct MaschineMk3UserClient_IVars {
    IOService *owner;                   // MaschineMk3HidTransport or ...DisplayTransport
    bool       isHid;                   // discriminator
    OSAction  *hidReportCompletion;     // set by REGISTER_HID_CALLBACK
    OSAction  *displayCompletion;       // set by REGISTER_DISPLAY_CALLBACK
    // Flow-control mirrors:
    uint32_t   hidOutInflight;
    uint32_t   displayInflight;
};
```

**Callback dispatch surface.** Two callbacks, both async via `OSAction` —
no shared-memory ring. See §3 for the full dispatch table.

---

## 3. IPC wire protocol

**Primitive chosen: `IOUserClient` with an `ExternalMethod` dispatch table,
plus one stashed `OSAction` per interface for async push.**

Rationale: it's the only DriverKit-supported IPC to userspace, the request
side is zero-copy struct-passing, and async `OSAction` completions are
exactly the right primitive for "hey, HID-IN report arrived." Shared-memory
rings were considered and rejected — at 64 B/report and ~1 kHz the scalar/
struct path comfortably fits, and shared memory adds cache-invalidation
concerns and a second synchronisation primitive. If we ever profile a
bottleneck in the HID path, we revisit.

For displays there is no inbound data at all; completions carry only
`(seq, status)` scalars.

### 3.1 Selector constants (in `MaschineIPC.h`)

```c
enum MaschineSelector {
    kSel_Open                    = 0,   // handshake + protocol version
    kSel_Close                   = 1,
    kSel_RegisterHidCallback     = 2,   // async: stashes OSAction for HID IN
    kSel_RegisterDisplayCallback = 3,   // async: stashes OSAction for display-done
    kSel_HidOutReport            = 4,   // struct: write a HID OUT report
    kSel_BulkOut                 = 5,   // struct: submit one display frame
    kSel_DeviceState             = 6,   // struct-out: static device info
    kSel_Abort                   = 7,   // scalar: abort inflight + clear stalls
    kMaschineSelectorCount       = 8,
};

#define MASCHINE_IPC_VERSION 1
#define MASCHINE_HID_REPORT_MAX   512
#define MASCHINE_BULK_FRAME_MAX   524288  // 512 kB headroom for 480x272*2B + prelude

typedef struct __attribute__((packed)) {
    uint32_t clientVersion;       // MASCHINE_IPC_VERSION
    uint32_t flags;               // bit0 = want_display, bit1 = want_hid
} MaschineOpenIn;

typedef struct __attribute__((packed)) {
    uint32_t dextVersion;
    uint32_t vendorId;            // 0x17CC
    uint32_t productId;           // 0x1600
    uint8_t  interfaceNumber;     // 4 or 5
    uint8_t  _pad[3];
} MaschineOpenOut;

typedef struct __attribute__((packed)) {
    uint32_t length;
    uint8_t  data[MASCHINE_HID_REPORT_MAX];   // first `length` bytes are the report
} MaschineHidOut;

typedef struct __attribute__((packed)) {
    uint32_t length;
    uint32_t seq;                 // userspace-assigned; echoed in completion
    uint8_t  data[MASCHINE_BULK_FRAME_MAX];
} MaschineBulkOut;

typedef struct __attribute__((packed)) {
    uint8_t  vendorId[2];
    uint8_t  productId[2];
    uint8_t  bInterfaceNumber;
    uint8_t  epInAddr, epOutAddr, epBulkAddr;
    uint16_t inMaxPacket, outMaxPacket, bulkMaxPacket;
} MaschineDeviceState;

// Payload of the async HID-IN callback (scalars only; arg0=status, arg1=seq,
// arg2=length, then a 64-byte inline struct carried as structureOutput).
typedef struct __attribute__((packed)) {
    uint32_t length;
    uint32_t seq;
    uint64_t timestamp;           // mach_absolute_time from CompleteAsyncIO
    uint8_t  data[64];            // HID IN reports on Mk3 are always <=64
} MaschineHidInEvent;
```

### 3.2 `ExternalMethodDispatch` table

```cpp
static const IOUserClientMethodDispatch gMethods[kMaschineSelectorCount] = {
  [kSel_Open] = {
      .function                = (IOUserClientMethodFunction)&MaschineMk3UserClient::Static_Open,
      .checkCompletionExists   = false,
      .checkScalarInputCount   = 0,
      .checkStructureInputSize = sizeof(MaschineOpenIn),
      .checkScalarOutputCount  = 0,
      .checkStructureOutputSize= sizeof(MaschineOpenOut),
  },
  [kSel_Close] = { &Static_Close, false, 0, 0, 0, 0 },
  [kSel_RegisterHidCallback] = {
      .function                = &Static_RegisterHidCallback,
      .checkCompletionExists   = true,      // async — completion is the push channel
      .checkScalarInputCount   = 0,
      .checkStructureInputSize = 0,
      .checkScalarOutputCount  = 0,
      .checkStructureOutputSize= 0,
  },
  [kSel_RegisterDisplayCallback] = {
      .function                = &Static_RegisterDisplayCallback,
      .checkCompletionExists   = true,
      .checkScalarInputCount   = 0,
      .checkStructureInputSize = 0,
      .checkScalarOutputCount  = 0,
      .checkStructureOutputSize= 0,
  },
  [kSel_HidOutReport] = {
      .function                = &Static_HidOutReport,
      .checkCompletionExists   = false,
      .checkScalarInputCount   = 0,
      .checkStructureInputSize = kIOUserClientVariableStructureSize,  // 4 ≤ n ≤ 516
      .checkScalarOutputCount  = 0,
      .checkStructureOutputSize= 0,
  },
  [kSel_BulkOut] = {
      .function                = &Static_BulkOut,
      .checkCompletionExists   = false,
      .checkScalarInputCount   = 0,
      .checkStructureInputSize = kIOUserClientVariableStructureSize,
      .checkScalarOutputCount  = 0,
      .checkStructureOutputSize= 0,
  },
  [kSel_DeviceState] = {
      .function                = &Static_DeviceState,
      .checkCompletionExists   = false,
      .checkScalarInputCount   = 0,
      .checkStructureInputSize = 0,
      .checkScalarOutputCount  = 0,
      .checkStructureOutputSize= sizeof(MaschineDeviceState),
  },
  [kSel_Abort] = {
      .function                = &Static_Abort,
      .checkCompletionExists   = false,
      .checkScalarInputCount   = 1,   // scalar0 = kAbortAll | kAbortHidOut | kAbortDisplay
      .checkStructureInputSize = 0,
      .checkScalarOutputCount  = 0,
      .checkStructureOutputSize= 0,
  },
};

kern_return_t MaschineMk3UserClient::ExternalMethod(uint64_t selector,
        IOUserClientMethodArguments *args,
        const IOUserClientMethodDispatch *dispatch,
        OSObject *target, void *reference) {
    if (selector >= kMaschineSelectorCount) return kIOReturnBadArgument;
    return super::ExternalMethod(selector, args, &gMethods[selector],
                                 target ? target : this, reference);
}
```

### 3.3 Async push choice

**HID IN reports push back via a single stashed `OSAction` + `AsyncCompletion`,
not shared memory and not a polled `HID_IN_POLL`.** Chosen because (a) the
DriverKit SDK provides this path first-class, (b) userspace sees it as a
blocking `IOConnectCallAsyncStructMethod` that wakes via `IODataQueue`-less
mach-port notification, (c) there's no "poll" latency tax — the report is in
userspace within microseconds of the USB completion.

Flow on the dext side:
```cpp
// In Static_RegisterHidCallback:
ivars->hidReportCompletion = args->completion;
ivars->hidReportCompletion->retain();
return kIOReturnSuccess;

// From MaschineMk3HidTransport::OnHidReadComplete:
uint64_t scalars[2] = { seq, (uint64_t)actualBytes };
args->structureOutput = OSData::withBytes(buf->getBytesNoCopy(), actualBytes);
AsyncCompletion(ivars->hidReportCompletion, kIOReturnSuccess,
                scalars, /*count*/ 2);
```

Display completion uses the same machinery with `displayCompletion` —
just `(seq, status)` scalars, no structure.

### 3.4 Flow control

| Channel | Limit | Enforcement |
|---|---|---|
| HID OUT | 8 in flight | `MaschineMk3HidTransport` ring; full ⇒ `kIOReturnNoResources` → Rust maps to `TransportError::Busy` and awaits next completion |
| Display | 16 in flight | Same pattern on `MaschineMk3DisplayTransport` |
| HID IN | 1 in flight, always requeued | Zero user-space control — the dext owns the read loop |

These limits are internal to the dext and enforced synchronously on the
`ExternalMethod` call. No out-of-band "credits" exchange. On the Rust
side, `DextTransport` uses a Tokio `Semaphore(8)` for HID OUT and
`Semaphore(16)` for display to mirror the dext — avoids almost all
synchronous `NoResources` returns and gives cooperative backpressure.

### 3.5 Error propagation

`ExternalMethod` implementations return `kern_return_t`. On the Rust side,
`IOConnectCallStructMethod` returns that value; we translate at a single
choke point in `dext.rs`:

```rust
fn map_kr(kr: kern_return_t) -> Result<(), TransportError> {
    match kr {
        KERN_SUCCESS                => Ok(()),
        k if k == kIOReturnNoResources => Err(TransportError::Busy),
        k if k == kIOReturnNotOpen  => Err(TransportError::Closed),
        k if k == kIOReturnAborted  => Err(TransportError::Aborted),
        k if k == kIOUSBPipeStalled => Err(TransportError::Stalled),
        k                           => Err(TransportError::Ioctl(k)),
    }
}
```

Async completions deliver a `kern_return_t` in the first scalar; the same
mapping applies. No `IOReturn` value is allowed to leak past `dext.rs`.

### 3.6 Full message list

| Message | Direction | Selector | Sync/Async | Notes |
|---|---|---|---|---|
| `OPEN` | userspace → dext | `kSel_Open` | sync struct | protocol handshake, per connection |
| `CLOSE` | userspace → dext | `kSel_Close` | sync scalar | idempotent |
| `HID_IN` push | dext → userspace | `kSel_RegisterHidCallback` completion | async | carries `(seq, len, bytes)` |
| `HID_OUT_REPORT` | userspace → dext | `kSel_HidOutReport` | sync struct | payload ≤ 512 B |
| `BULK_OUT` | userspace → dext | `kSel_BulkOut` | sync struct | payload ≤ 512 kB |
| `DISPLAY_DONE` push | dext → userspace | `kSel_RegisterDisplayCallback` completion | async | carries `(seq, status)` |
| `DEVICE_STATE` | userspace → dext | `kSel_DeviceState` | sync struct | static descriptor mirror |
| `ABORT` | userspace → dext | `kSel_Abort` | sync scalar | used during shutdown or after stall |

---

## 4. Rust side — `crates/maschine-core/src/transport.rs`

The current `Transport` becomes an enum façade; the `nusb` body moves into
`transport/nusb.rs`; a new `transport/dext.rs` adds `DextTransport`.

### 4.1 New shape

```rust
pub enum Transport {
    #[cfg(all(target_os = "macos", feature = "dext"))]
    Dext(dext::DextTransport),
    Nusb(nusb_impl::NusbTransport),    // existing body
}

impl Transport {
    pub async fn open() -> Result<Self, TransportError> {
        #[cfg(all(target_os = "macos", feature = "dext"))]
        if std::env::var_os("MASCHINE_FORCE_NUSB").is_none() {
            match dext::DextTransport::open().await {
                Ok(t)  => return Ok(Transport::Dext(t)),
                Err(e) => tracing::warn!("dext unavailable ({e}); falling back to nusb"),
            }
        }
        Ok(Transport::Nusb(nusb_impl::NusbTransport::open().await?))
    }

    pub fn spawn_hid_reader(&self) -> InboundRx { /* delegate */ }
    pub async fn write_hid(&self, payload: Vec<u8>) -> Result<(), TransportError> { /* delegate */ }
    pub async fn write_display(&self, payload: Vec<u8>) -> Result<(), TransportError> { /* delegate */ }
    pub fn has_display(&self) -> bool { /* delegate */ }
}
```

### 4.2 `DextTransport` internals

```rust
#[cfg(target_os = "macos")]
pub struct DextTransport {
    hid_conn:     io_connect_t,     // one IOConnect per user client
    display_conn: io_connect_t,
    hid_inbound:  mpsc::Receiver<Vec<u8>>,  // fed by IONotificationPort thread
    hid_write_slots:     Arc<Semaphore>,    // 8
    display_write_slots: Arc<Semaphore>,    // 16
    _notify_thread: JoinHandle<()>,
}
```

**Open path:**
1. `IOServiceGetMatchingServices` with `IOServiceNameMatching("MaschineMk3HidTransport")`; take the first.
2. `IOServiceOpen(service, mach_task_self_, 0, &hid_conn)`.
3. `IOConnectCallStructMethod(hid_conn, kSel_Open, &in, sizeof(in), &out, &outSize)` — verifies protocol version.
4. Same three steps for `MaschineMk3DisplayTransport` → `display_conn`.
5. Create an `IONotificationPortRef`, pull its `mach_port_t`, hand it to both user clients by calling `IOConnectCallAsyncStructMethod(kSel_RegisterHidCallback, …)` with `wakePort = port` and a `(self_ref, callback_ptr)` async reference. Do the same for the display callback on the other connection.
6. Spawn a dedicated OS thread that creates a `CFRunLoop`, adds the notification port's run-loop source, and runs forever. The C callback posts decoded `MaschineHidInEvent` onto `hid_inbound` and display-done sequences onto a second channel.

**FFI surface (wrapped in `io-kit-sys` + `mach2` — both already usable from
the workspace; no patched dependencies needed):**

```rust
extern "C" {
    fn IOServiceOpen(service: io_service_t, owningTask: task_port_t,
                     type_: u32, connect: *mut io_connect_t) -> kern_return_t;
    fn IOConnectCallStructMethod(connection: io_connect_t, selector: u32,
        inputStruct: *const c_void, inputStructCnt: usize,
        outputStruct: *mut c_void, outputStructCnt: *mut usize) -> kern_return_t;
    fn IOConnectCallAsyncStructMethod(connection: io_connect_t, selector: u32,
        wakePort: mach_port_t, reference: *const u64, referenceCnt: u32,
        inputStruct: *const c_void, inputStructCnt: usize,
        outputStruct: *mut c_void, outputStructCnt: *mut usize) -> kern_return_t;
    fn IOServiceClose(connection: io_connect_t) -> kern_return_t;
}
```

### 4.3 Feature gates

- `maschine-core` gains feature `dext` (default off in workspace tier,
  default on in the release `.app` build script).
- `DextTransport` is fenced with `#[cfg(all(target_os = "macos", feature = "dext"))]`.
- Linux and Windows compile only the `nusb_impl` path.
- The workspace gains a sibling crate (or module) `crates/maschine-proto/src/dext_ipc.rs`
  containing `#[repr(C, packed)]` mirrors of `MaschineIPC.h` and
  `const` selector IDs, so there is exactly one source of truth (C header)
  and one parallel Rust mirror; a CI test verifies struct sizes by
  `assert_eq!(mem::size_of::<MaschineHidOut>(), 516)`.

### 4.4 Behavioural parity with nusb path

`spawn_hid_reader` on `DextTransport` drains from the
`IONotificationPort`-fed channel into the same `mpsc<Vec<u8>>` the nusb path
produces. No call site above `Transport` needs to care which backend is
live. On dext, the existing `platform::current().prepare()` does nothing
— no `kickstart` agent gymnastics required.

---

## 5. Build system

**Cargo does NOT invoke xcodebuild automatically.** `build.rs` in
`maschine-core` only exposes `cfg(has_dext=true)` when the env var
`MASCHINE_DEXT_BUILT=1` is present, which the top-level `dext/build.sh`
sets after a successful `xcodebuild`. A `cargo build` in isolation never
touches Xcode. Rationale: xcodebuild is slow (~10s minimum cold), requires
provisioning profiles present on the developer's Mac, needs signing
identity access, and can prompt the keychain — none of which belong on
the default Rust build path. A developer editing only Rust shouldn't pay
that tax every `cargo check`.

**Canonical build entry point** is `dext/build.sh`:

```bash
#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")"
CFG="${CONFIG:-Debug}"
xcodebuild -project Maschine.xcodeproj -scheme MaschineDext -configuration "$CFG" \
           -derivedDataPath build SYMROOT="$PWD/build/sym"
xcodebuild -project Maschine.xcodeproj -scheme MaschineHost -configuration "$CFG" \
           -derivedDataPath build SYMROOT="$PWD/build/sym"
# Copy the Rust daemon into the .app
cargo build --release -p maschined
cp ../target/release/maschined build/sym/$CFG/Maschine.app/Contents/MacOS/
# Copy the dext under the host app
mkdir -p build/sym/$CFG/Maschine.app/Contents/Library/SystemExtensions
cp -R build/sym/$CFG/MaschineDext.dext \
      build/sym/$CFG/Maschine.app/Contents/Library/SystemExtensions/
MASCHINE_DEXT_BUILT=1 cargo check -p maschine-core --features dext   # sanity
```

For release: `CONFIG=Release dext/build.sh && dext/scripts/sign.sh &&
dext/scripts/pkgbuild.sh && dext/scripts/notarize.sh` — the exact recipe
in R2.

**CI.** GitHub Actions runs Linux builds (`cargo test`) without touching
the dext. A separate self-hosted macOS-15 runner with SIP disabled and
the dev provisioning profile installed runs `dext/build.sh Debug` +
`cargo test --features dext -p maschine-core` + `cargo test --workspace`.
Notarisation runs from a tagged release workflow only; it needs the
`App Store Connect API` secret stashed in Actions secrets and is not part
of the per-commit check.

We use the Xcode project format (not CMake, not SPM+Makefile). Reasons:
Xcode is the only signing pipeline that understands DriverKit
provisioning profiles without custom tooling; IIG compilation is driven
by Xcode build rules and painful to reproduce in Make; every reference
project we fork from (USBApp, DriverKitUserClientSample) already ships an
Xcode project. CMake would be re-inventing what Xcode already does
correctly.

---

## 6. Milestone breakdown

Ordering: A2 (scaffold) → M1 → M2 → I2 (Rust side begins in parallel with
M3) → M3 → M4 → M5. I1 (dext plumbing) is the superset of M1–M4; I2 (Rust
DextTransport) is the superset of the Rust side used by M2/M3/M4.

| Milestone | Exit criterion | Rough LOC |
|---|---|---|
| **M1 — dext loads and logs** | `systemextensionsctl list` shows `com.cantonic.maschine.dext` in state `activated enabled`. Plugging the Mk3 emits an `os_log` "MaschineMk3HidTransport Start succeeded" from the dext. No IPC yet. | ~400 LOC C++ (two empty `Start`/`Stop` pairs, `Info.plist`, entitlements, host `main.swift`, build.sh) |
| **M2 — HID IN reports reach Rust** | Pressing a pad on the Mk3 produces a log line in `maschined` via `DextTransport::spawn_hid_reader`. Round-trip latency ≤ 2 ms observed. | ~900 LOC total: +300 C++ (`MaschineMk3UserClient`, HID read loop + async completion), +600 Rust (`dext.rs` open + register_hid_callback + notification thread + selector constants) |
| **M3 — HID OUT reports land** | `maschined` lights a specific RGB pad via a synthesised OUT report; verified on hardware. HID OUT ring + backpressure working. | ~300 LOC: +150 C++ (OUT ring, `Static_HidOutReport`), +150 Rust (`write_hid`, `Semaphore`) |
| **M4 — display bulk works** | A constant test pattern renders on both Mk3 displays at ≥30 fps, sustained, with no dropped frames over a 60 s soak. | ~500 LOC: +300 C++ (`MaschineMk3DisplayTransport`, ring, AsyncIOBundled), +200 Rust (`write_display`, display-done channel) |
| **M5 — signed + notarised installer** | `Maschine-0.1.0.pkg` passes `xcrun stapler validate` and installs on a clean macOS 15.x user account without dev-mode, activating the dext via the Sequoia Extensions UI. | ~100 LOC shell (scripts/*.sh) + entitlements review; most of the work is Apple-side lead time (R2 §2.3) |

Total new code: roughly **2 000 LOC C++/Obj-C++ and 1 000 LOC Rust**, plus
~300 LOC shell + plist. No changes outside `dext/` and
`crates/maschine-{core,proto}/`.

I1 task in the task list covers M1–M4 on the C++ side; I2 task covers the
Rust side of M2–M4. P1 is M5.

---

## 7. Risks and open questions

Three decisions I couldn't resolve without hardware or without an Apple
entitlement grant. Flagging for the project owner.

1. **Does if#5 use one bulk-OUT pipe with two logical displays multiplexed,
   or two distinct pipes (0x05 and 0x06)?** R1 §3 says "to be confirmed at
   bringup." The current nusb code uses a single `EP_DISPLAY_OUT=0x04` on
   the display interface, which contradicts R1's "0x05/0x06" note — and
   this conflict is load-bearing for `MaschineMk3DisplayTransport`
   (one `IOUSBHostPipe*` vs two, one 16-slot ring vs two 8-slot rings).
   The dext target should probe the descriptor in `Start()` and adapt.
   **Action needed from you before I1:** confirm by running a one-off
   `system_profiler SPUSBDataType` on a connected Mk3 and paste the
   interface-5 endpoint stanza. I'll update the ivars and ring logic
   accordingly — the difference is ~20 LOC either way but changes the
   testing plan for M4.

2. **Should the host `.app` launch `maschined` itself, or should
   `maschined` remain a LaunchAgent installed by the `.pkg`?** Karabiner
   uses a LaunchDaemon + an Application shim (R3 §3) — clean, survives app
   quit, but requires a `launchctl` plist and Full Disk Access prompts.
   Launching from `main.swift` is simpler but the daemon dies when the
   user quits the app. I've scaffolded the simpler option (launched from
   `main.swift` with `NSRunningApplication`), but if the v0.1 target is
   "user opens DAW, controller works, user closes DAW, controller still
   works from another DAW" then we need the LaunchDaemon variant. Please
   confirm which behaviour you want — this only affects `dext/MaschineHost/`
   and `dext/scripts/pkgbuild.sh`, not the dext or Rust core.

3. **Apple entitlement for `driverkit.userclient-access` naming a VID we
   don't own.** R2 §2 flags this as the highest-risk rejection axis. If
   Apple comes back asking for an NI letter of no-objection, the whole
   M5 notarisation milestone slips 2+ months or requires a pivot to
   "requires dev-mode + SIP-off" shipping. The dext architecture doesn't
   change either way, but the P1 task plan does. Decide now whether you
   want to file the entitlement request *this week* (blocks nothing on our
   side, starts the clock) or wait until M3 is demoable.

None of these block M1 or M2. M1 can start today using the
`.development` entitlement and SIP off.
