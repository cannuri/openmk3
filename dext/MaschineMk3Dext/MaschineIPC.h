//
// MaschineIPC.h
//
// Canonical wire protocol between the Maschine dext and the Rust user-space
// client. Mirrored byte-for-byte in crates/maschine-core/src/transport/dext_wire.rs.
// Authority: dext/docs/A1-architecture.md §3.1.
//
// Sizes verified by `#[cfg(test)] size_of` asserts in dext_wire.rs:
//   MaschineOpenIn       = 8
//   MaschineOpenOut      = 16
//   MaschineHidOut       = 4 + 512          = 516
//   MaschineBulkOut      = 4 + 4 + 524288   = 524296
//   MaschineDeviceState  = 14
//   MaschineHidInEvent   = 4 + 4 + 8 + 64   = 80
//

#ifndef MaschineIPC_h
#define MaschineIPC_h

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

enum MaschineSelector {
    kSel_Open                    = 0,
    kSel_Close                   = 1,
    kSel_RegisterHidCallback     = 2,
    kSel_RegisterDisplayCallback = 3,
    kSel_HidOutReport            = 4,
    kSel_BulkOut                 = 5,
    kSel_DeviceState             = 6,
    kSel_Abort                   = 7,
    kMaschineSelectorCount       = 8,
};

#define MASCHINE_IPC_VERSION      1u
#define MASCHINE_HID_REPORT_MAX   512u
#define MASCHINE_BULK_FRAME_MAX   524288u

#define MASCHINE_OPEN_FLAG_WANT_DISPLAY (1u << 0)
#define MASCHINE_OPEN_FLAG_WANT_HID     (1u << 1)

#define MASCHINE_ABORT_ALL     0u
#define MASCHINE_ABORT_HID_OUT 1u
#define MASCHINE_ABORT_DISPLAY 2u

#define MASCHINE_HID_IN_EP   0x84u
#define MASCHINE_HID_OUT_EP  0x03u
#define MASCHINE_BULK_OUT_EP 0x04u

#define MASCHINE_HID_IN_PACKET_MAX  64u
#define MASCHINE_HID_OUT_RING_DEPTH 4u
#define MASCHINE_DISPLAY_RING_DEPTH 16u

typedef struct __attribute__((packed)) {
    uint32_t clientVersion;
    uint32_t flags;
} MaschineOpenIn;

typedef struct __attribute__((packed)) {
    uint32_t dextVersion;
    uint32_t vendorId;
    uint32_t productId;
    uint8_t  interfaceNumber;
    uint8_t  _pad[3];
} MaschineOpenOut;

typedef struct __attribute__((packed)) {
    uint32_t length;
    uint8_t  data[MASCHINE_HID_REPORT_MAX];
} MaschineHidOut;

typedef struct __attribute__((packed)) {
    uint32_t length;
    uint32_t seq;
    uint8_t  data[MASCHINE_BULK_FRAME_MAX];
} MaschineBulkOut;

typedef struct __attribute__((packed)) {
    uint8_t  vendorId[2];
    uint8_t  productId[2];
    uint8_t  bInterfaceNumber;
    uint8_t  epInAddr;
    uint8_t  epOutAddr;
    uint8_t  epBulkAddr;
    uint16_t inMaxPacket;
    uint16_t outMaxPacket;
    uint16_t bulkMaxPacket;
} MaschineDeviceState;

typedef struct __attribute__((packed)) {
    uint32_t length;
    uint32_t seq;
    uint64_t timestamp;
    uint8_t  data[64];
} MaschineHidInEvent;

#ifdef __cplusplus
} // extern "C"
#endif

#endif /* MaschineIPC_h */
