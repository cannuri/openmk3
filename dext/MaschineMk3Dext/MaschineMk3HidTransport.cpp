//
// MaschineMk3HidTransport.cpp
//
// Opens if#4, copies the interrupt IN (0x84) and OUT (0x03) pipes, runs a
// single in-flight IN read loop, and serves up to 4 concurrent OUT reports.
// The IN completion forwards each 42-byte frame to the attached user client,
// which surfaces it to Rust via a stashed async OSAction.
//

#include <os/log.h>
#include <stdatomic.h>
#include <DriverKit/IOUserServer.h>
#include <DriverKit/IOLib.h>
#include <DriverKit/OSAction.h>
#include <DriverKit/IOBufferMemoryDescriptor.h>
#include <DriverKit/IODispatchQueue.h>
#include <DriverKit/IOReturn.h>
#include <USBDriverKit/IOUSBHostInterface.h>
#include <USBDriverKit/IOUSBHostPipe.h>
#include <USBDriverKit/IOUSBHostFamilyDefinitions.h>

#include "MaschineIPC.h"
#include "MaschineMk3HidTransport.h"
#include "MaschineMk3UserClient.h"

#define kHidInBufferSize  MASCHINE_HID_IN_PACKET_MAX
#define kHidOutBufferSize MASCHINE_HID_REPORT_MAX
#define kHidOutRingDepth  MASCHINE_HID_OUT_RING_DEPTH

struct MaschineMk3HidTransport_IVars {
    IOUSBHostInterface       * interface       = nullptr;
    IOUSBHostPipe            * hidInPipe       = nullptr;
    IOUSBHostPipe            * hidOutPipe      = nullptr;

    IOBufferMemoryDescriptor * inBuf           = nullptr;
    OSAction                 * inReadAction    = nullptr;

    IOBufferMemoryDescriptor * outBuf[kHidOutRingDepth]    = { nullptr };
    OSAction                 * outAction[kHidOutRingDepth] = { nullptr };
    _Atomic uint32_t           outSlotFree;   // bitmask: bit i set == slot i free

    uint8_t                    interfaceNumber = 4;
    bool                       interfaceOpen   = false;
    _Atomic uint32_t           seqCounter;

    MaschineMk3UserClient    * userClient      = nullptr;
};

bool MaschineMk3HidTransport::init()
{
    if (!super::init()) {
        return false;
    }
    ivars = IONewZero(MaschineMk3HidTransport_IVars, 1);
    if (ivars == nullptr) {
        return false;
    }
    atomic_init(&ivars->outSlotFree, (1u << kHidOutRingDepth) - 1u);
    atomic_init(&ivars->seqCounter, 0u);
    return true;
}

void MaschineMk3HidTransport::free()
{
    IOSafeDeleteNULL(ivars, MaschineMk3HidTransport_IVars, 1);
    super::free();
}

// Find the first free slot and atomically claim it. Returns -1 on ring full.
static int claim_out_slot(_Atomic uint32_t * mask)
{
    uint32_t cur = atomic_load(mask);
    while (cur != 0) {
        uint32_t bit = cur & (-cur);                        // lowest set bit
        uint32_t next = cur & ~bit;
        if (atomic_compare_exchange_weak(mask, &cur, next)) {
            return __builtin_ctz(bit);
        }
    }
    return -1;
}

static void release_out_slot(_Atomic uint32_t * mask, uint32_t slot)
{
    atomic_fetch_or(mask, 1u << slot);
}

kern_return_t IMPL(MaschineMk3HidTransport, Start)
{
    kern_return_t ret = Start(provider, SUPERDISPATCH);
    if (ret != kIOReturnSuccess) {
        os_log(OS_LOG_DEFAULT, "MaschineMk3HidTransport super::Start failed: 0x%08x", ret);
        return ret;
    }

    ivars->interface = OSDynamicCast(IOUSBHostInterface, provider);
    if (ivars->interface == nullptr) {
        os_log(OS_LOG_DEFAULT, "MaschineMk3HidTransport: provider is not IOUSBHostInterface");
        Stop(provider, SUPERDISPATCH);
        return kIOReturnNoDevice;
    }

    ret = ivars->interface->Open(this, 0, nullptr);
    if (ret != kIOReturnSuccess) {
        os_log(OS_LOG_DEFAULT, "MaschineMk3HidTransport: Open failed: 0x%08x", ret);
        Stop(provider, SUPERDISPATCH);
        return ret;
    }
    ivars->interfaceOpen = true;

    ret = ivars->interface->CopyPipe(MASCHINE_HID_IN_EP, &ivars->hidInPipe);
    if (ret != kIOReturnSuccess || ivars->hidInPipe == nullptr) {
        os_log(OS_LOG_DEFAULT, "MaschineMk3HidTransport: CopyPipe(0x84) failed: 0x%08x", ret);
        Stop(provider, SUPERDISPATCH);
        return ret;
    }

    ret = ivars->interface->CopyPipe(MASCHINE_HID_OUT_EP, &ivars->hidOutPipe);
    if (ret != kIOReturnSuccess || ivars->hidOutPipe == nullptr) {
        os_log(OS_LOG_DEFAULT, "MaschineMk3HidTransport: CopyPipe(0x03) failed: 0x%08x", ret);
        Stop(provider, SUPERDISPATCH);
        return ret;
    }

    ret = ivars->interface->CreateIOBuffer(kIOMemoryDirectionIn, kHidInBufferSize, &ivars->inBuf);
    if (ret != kIOReturnSuccess || ivars->inBuf == nullptr) {
        os_log(OS_LOG_DEFAULT, "MaschineMk3HidTransport: CreateIOBuffer(in) failed: 0x%08x", ret);
        Stop(provider, SUPERDISPATCH);
        return ret;
    }
    ivars->inBuf->SetLength(kHidInBufferSize);

    ret = CreateActionHandleHidInComplete(sizeof(uint32_t), &ivars->inReadAction);
    if (ret != kIOReturnSuccess || ivars->inReadAction == nullptr) {
        os_log(OS_LOG_DEFAULT, "MaschineMk3HidTransport: CreateActionHandleHidInComplete failed: 0x%08x", ret);
        Stop(provider, SUPERDISPATCH);
        return ret;
    }

    for (uint32_t i = 0; i < kHidOutRingDepth; i++) {
        ret = ivars->interface->CreateIOBuffer(kIOMemoryDirectionOut, kHidOutBufferSize, &ivars->outBuf[i]);
        if (ret != kIOReturnSuccess || ivars->outBuf[i] == nullptr) {
            os_log(OS_LOG_DEFAULT, "MaschineMk3HidTransport: CreateIOBuffer(out %u) failed: 0x%08x", i, ret);
            Stop(provider, SUPERDISPATCH);
            return ret;
        }
        ret = CreateActionHandleHidOutComplete(sizeof(uint32_t), &ivars->outAction[i]);
        if (ret != kIOReturnSuccess || ivars->outAction[i] == nullptr) {
            os_log(OS_LOG_DEFAULT, "MaschineMk3HidTransport: CreateActionHandleHidOutComplete(%u) failed: 0x%08x", i, ret);
            Stop(provider, SUPERDISPATCH);
            return ret;
        }
        // Stash the slot index in the OSAction's per-reference storage so
        // the completion knows which slot to release.
        uint32_t * slotRef = (uint32_t *)ivars->outAction[i]->GetReference();
        if (slotRef != nullptr) {
            *slotRef = i;
        }
    }

    // Kick the IN read loop.
    ret = ivars->hidInPipe->AsyncIO(ivars->inBuf, kHidInBufferSize, ivars->inReadAction, 0);
    if (ret != kIOReturnSuccess) {
        os_log(OS_LOG_DEFAULT, "MaschineMk3HidTransport: initial AsyncIO failed: 0x%08x", ret);
        Stop(provider, SUPERDISPATCH);
        return ret;
    }

    os_log(OS_LOG_DEFAULT, "MaschineMk3HidTransport::Start succeeded — Mk3 HID interface (if#4) attached");
    RegisterService();
    return kIOReturnSuccess;
}

kern_return_t IMPL(MaschineMk3HidTransport, Stop)
{
    os_log(OS_LOG_DEFAULT, "MaschineMk3HidTransport::Stop — detaching");

    if (ivars->hidInPipe) {
        ivars->hidInPipe->Abort(0, kIOReturnAborted, nullptr);
    }
    if (ivars->hidOutPipe) {
        ivars->hidOutPipe->Abort(0, kIOReturnAborted, nullptr);
    }

    OSSafeReleaseNULL(ivars->inReadAction);
    OSSafeReleaseNULL(ivars->inBuf);
    for (uint32_t i = 0; i < kHidOutRingDepth; i++) {
        OSSafeReleaseNULL(ivars->outAction[i]);
        OSSafeReleaseNULL(ivars->outBuf[i]);
    }
    OSSafeReleaseNULL(ivars->hidInPipe);
    OSSafeReleaseNULL(ivars->hidOutPipe);

    if (ivars->interface && ivars->interfaceOpen) {
        ivars->interface->Close(this, 0);
        ivars->interfaceOpen = false;
    }
    ivars->interface = nullptr;

    return Stop(provider, SUPERDISPATCH);
}

void IMPL(MaschineMk3HidTransport, HandleHidInComplete)
{
    if (status == kIOReturnAborted) {
        // Interface is shutting down — don't requeue.
        return;
    }

    if (status == kUSBHostReturnPipeStalled) {
        os_log(OS_LOG_DEFAULT, "MaschineMk3HidTransport: IN stalled, clearing");
        if (ivars->hidInPipe) {
            ivars->hidInPipe->ClearStall(true);
        }
    } else if (status == kIOReturnSuccess && actualByteCount > 0) {
        uint32_t seq = atomic_fetch_add(&ivars->seqCounter, 1u) + 1u;
        MaschineMk3UserClient * client = ivars->userClient;
        if (client != nullptr && ivars->inBuf != nullptr) {
            IOAddressSegment seg = { 0, 0 };
            if (ivars->inBuf->GetAddressRange(&seg) == kIOReturnSuccess) {
                const uint8_t * bytes = (const uint8_t *)(uintptr_t)seg.address;
                client->DeliverHidIn(bytes, actualByteCount, seq, completionTimestamp);
            }
        }
    } else if (status != kIOReturnSuccess) {
        os_log(OS_LOG_DEFAULT, "MaschineMk3HidTransport: IN completion status 0x%08x (len=%u)", status, actualByteCount);
    }

    // Always requeue unless we're being torn down.
    if (ivars->hidInPipe != nullptr && ivars->inReadAction != nullptr) {
        kern_return_t kr = ivars->hidInPipe->AsyncIO(ivars->inBuf, kHidInBufferSize, ivars->inReadAction, 0);
        if (kr != kIOReturnSuccess && kr != kIOReturnAborted) {
            os_log(OS_LOG_DEFAULT, "MaschineMk3HidTransport: requeue AsyncIO failed: 0x%08x", kr);
        }
    }
}

void IMPL(MaschineMk3HidTransport, HandleHidOutComplete)
{
    const uint32_t * slotRef = (const uint32_t *)action->GetReference();
    uint32_t slot = (slotRef != nullptr) ? *slotRef : 0u;

    if (status != kIOReturnSuccess && status != kIOReturnAborted) {
        os_log(OS_LOG_DEFAULT, "MaschineMk3HidTransport: OUT completion slot=%u status=0x%08x", slot, status);
        if (status == kUSBHostReturnPipeStalled && ivars->hidOutPipe != nullptr) {
            ivars->hidOutPipe->ClearStall(true);
        }
    }
    release_out_slot(&ivars->outSlotFree, slot);
}

kern_return_t MaschineMk3HidTransport::SubmitHidOut(const void * bytes, uint32_t length)
{
    if (bytes == nullptr || length == 0 || length > kHidOutBufferSize) {
        return kIOReturnBadArgument;
    }
    if (ivars->hidOutPipe == nullptr) {
        return kIOReturnNotReady;
    }

    int slot = claim_out_slot(&ivars->outSlotFree);
    if (slot < 0) {
        return kIOReturnNoResources;
    }

    IOBufferMemoryDescriptor * buf = ivars->outBuf[slot];
    OSAction * act = ivars->outAction[slot];
    if (buf == nullptr || act == nullptr) {
        release_out_slot(&ivars->outSlotFree, (uint32_t)slot);
        return kIOReturnNotReady;
    }

    IOAddressSegment seg = { 0, 0 };
    kern_return_t gr = buf->GetAddressRange(&seg);
    if (gr != kIOReturnSuccess || seg.address == 0) {
        release_out_slot(&ivars->outSlotFree, (uint32_t)slot);
        return (gr != kIOReturnSuccess) ? gr : kIOReturnNotReady;
    }
    memcpy((void *)(uintptr_t)seg.address, bytes, length);
    buf->SetLength(length);

    kern_return_t kr = ivars->hidOutPipe->AsyncIO(buf, length, act, 0);
    if (kr != kIOReturnSuccess) {
        release_out_slot(&ivars->outSlotFree, (uint32_t)slot);
    }
    return kr;
}

kern_return_t MaschineMk3HidTransport::AbortHidOut()
{
    if (ivars->hidOutPipe == nullptr) {
        return kIOReturnNotReady;
    }
    return ivars->hidOutPipe->Abort(0, kIOReturnAborted, nullptr);
}

kern_return_t MaschineMk3HidTransport::AttachClient(MaschineMk3UserClient * client)
{
    ivars->userClient = client;
    return kIOReturnSuccess;
}

void MaschineMk3HidTransport::DetachClient(MaschineMk3UserClient * client)
{
    if (ivars->userClient == client) {
        ivars->userClient = nullptr;
    }
}

uint8_t MaschineMk3HidTransport::InterfaceNumber() const { return ivars->interfaceNumber; }
uint8_t MaschineMk3HidTransport::HidInEndpoint()   const { return MASCHINE_HID_IN_EP; }
uint8_t MaschineMk3HidTransport::HidOutEndpoint()  const { return MASCHINE_HID_OUT_EP; }
