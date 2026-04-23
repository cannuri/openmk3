//
// MaschineMk3DisplayTransport.cpp
//
// Opens if#5, copies the bulk-OUT pipe on EP 0x04, pre-allocates a ring of
// 16 buffers of MASCHINE_BULK_FRAME_MAX each, and serves submissions via
// AsyncIOBundled. Frame sequence numbers propagate back to the user client
// as display-done notifications so Rust can match completions to submissions.
//

#include <os/log.h>
#include <stdatomic.h>
#include <DriverKit/IOUserServer.h>
#include <DriverKit/IOLib.h>
#include <DriverKit/OSAction.h>
#include <DriverKit/IOBufferMemoryDescriptor.h>
#include <DriverKit/IOReturn.h>
#include <USBDriverKit/IOUSBHostInterface.h>
#include <USBDriverKit/IOUSBHostPipe.h>
#include <USBDriverKit/IOUSBHostFamilyDefinitions.h>

#include "MaschineIPC.h"
#include "MaschineMk3DisplayTransport.h"
#include "MaschineMk3UserClient.h"

#define kDisplayRingDepth MASCHINE_DISPLAY_RING_DEPTH
#define kDisplaySlotSize  MASCHINE_BULK_FRAME_MAX

struct MaschineMk3DisplayTransport_IVars {
    IOUSBHostInterface       * interface       = nullptr;
    IOUSBHostPipe            * bulkOutPipe     = nullptr;

    IOBufferMemoryDescriptor * slots[kDisplayRingDepth] = { nullptr };
    uint32_t                   slotSeq[kDisplayRingDepth] = { 0 };
    OSAction                 * bundledAction   = nullptr;
    IOLock                   * ringLock        = nullptr;

    uint8_t                    head            = 0;   // next index to submit
    uint8_t                    tail            = 0;   // next index to complete
    uint8_t                    inflight        = 0;
    bool                       interfaceOpen   = false;
    uint8_t                    interfaceNumber = 5;

    MaschineMk3UserClient    * userClient      = nullptr;
};

bool MaschineMk3DisplayTransport::init()
{
    if (!super::init()) {
        return false;
    }
    ivars = IONewZero(MaschineMk3DisplayTransport_IVars, 1);
    if (ivars == nullptr) {
        return false;
    }
    ivars->ringLock = IOLockAlloc();
    if (ivars->ringLock == nullptr) {
        IOSafeDeleteNULL(ivars, MaschineMk3DisplayTransport_IVars, 1);
        return false;
    }
    return true;
}

void MaschineMk3DisplayTransport::free()
{
    if (ivars && ivars->ringLock) {
        IOLockFree(ivars->ringLock);
        ivars->ringLock = nullptr;
    }
    IOSafeDeleteNULL(ivars, MaschineMk3DisplayTransport_IVars, 1);
    super::free();
}

kern_return_t IMPL(MaschineMk3DisplayTransport, Start)
{
    kern_return_t ret = Start(provider, SUPERDISPATCH);
    if (ret != kIOReturnSuccess) {
        os_log(OS_LOG_DEFAULT, "MaschineMk3DisplayTransport super::Start failed: 0x%08x", ret);
        return ret;
    }

    ivars->interface = OSDynamicCast(IOUSBHostInterface, provider);
    if (ivars->interface == nullptr) {
        os_log(OS_LOG_DEFAULT, "MaschineMk3DisplayTransport: provider is not IOUSBHostInterface");
        Stop(provider, SUPERDISPATCH);
        return kIOReturnNoDevice;
    }

    ret = ivars->interface->Open(this, 0, nullptr);
    if (ret != kIOReturnSuccess) {
        os_log(OS_LOG_DEFAULT, "MaschineMk3DisplayTransport: Open failed: 0x%08x", ret);
        Stop(provider, SUPERDISPATCH);
        return ret;
    }
    ivars->interfaceOpen = true;

    ret = ivars->interface->CopyPipe(MASCHINE_BULK_OUT_EP, &ivars->bulkOutPipe);
    if (ret != kIOReturnSuccess || ivars->bulkOutPipe == nullptr) {
        os_log(OS_LOG_DEFAULT, "MaschineMk3DisplayTransport: CopyPipe(0x04) failed: 0x%08x", ret);
        Stop(provider, SUPERDISPATCH);
        return ret;
    }

    ret = ivars->bulkOutPipe->CreateMemoryDescriptorRing(kDisplayRingDepth);
    if (ret != kIOReturnSuccess) {
        os_log(OS_LOG_DEFAULT, "MaschineMk3DisplayTransport: CreateMemoryDescriptorRing failed: 0x%08x", ret);
        Stop(provider, SUPERDISPATCH);
        return ret;
    }

    for (uint32_t i = 0; i < kDisplayRingDepth; i++) {
        ret = ivars->interface->CreateIOBuffer(kIOMemoryDirectionOut, kDisplaySlotSize, &ivars->slots[i]);
        if (ret != kIOReturnSuccess || ivars->slots[i] == nullptr) {
            os_log(OS_LOG_DEFAULT, "MaschineMk3DisplayTransport: CreateIOBuffer(slot %u) failed: 0x%08x", i, ret);
            Stop(provider, SUPERDISPATCH);
            return ret;
        }
        ret = ivars->bulkOutPipe->SetMemoryDescriptor(ivars->slots[i], i);
        if (ret != kIOReturnSuccess) {
            os_log(OS_LOG_DEFAULT, "MaschineMk3DisplayTransport: SetMemoryDescriptor(%u) failed: 0x%08x", i, ret);
            Stop(provider, SUPERDISPATCH);
            return ret;
        }
    }

    ret = CreateActionHandleBulkOutComplete(0, &ivars->bundledAction);
    if (ret != kIOReturnSuccess || ivars->bundledAction == nullptr) {
        os_log(OS_LOG_DEFAULT, "MaschineMk3DisplayTransport: CreateActionHandleBulkOutComplete failed: 0x%08x", ret);
        Stop(provider, SUPERDISPATCH);
        return ret;
    }

    os_log(OS_LOG_DEFAULT, "MaschineMk3DisplayTransport::Start succeeded — Mk3 display interface (if#5) attached");
    RegisterService();
    return kIOReturnSuccess;
}

kern_return_t IMPL(MaschineMk3DisplayTransport, Stop)
{
    os_log(OS_LOG_DEFAULT, "MaschineMk3DisplayTransport::Stop — detaching");

    if (ivars->bulkOutPipe) {
        ivars->bulkOutPipe->Abort(0, kIOReturnAborted, nullptr);
    }
    OSSafeReleaseNULL(ivars->bundledAction);
    for (uint32_t i = 0; i < kDisplayRingDepth; i++) {
        OSSafeReleaseNULL(ivars->slots[i]);
    }
    OSSafeReleaseNULL(ivars->bulkOutPipe);

    if (ivars->interface && ivars->interfaceOpen) {
        ivars->interface->Close(this, 0);
        ivars->interfaceOpen = false;
    }
    ivars->interface = nullptr;

    return Stop(provider, SUPERDISPATCH);
}

void IMPL(MaschineMk3DisplayTransport, HandleBulkOutComplete)
{
    (void)actualByteCountArrayCount;
    (void)statusArrayCount;

    MaschineMk3UserClient * client = nullptr;
    uint32_t doneSeq[kDisplayRingDepth];
    kern_return_t doneStatus[kDisplayRingDepth];
    uint32_t doneCount = 0;

    IOLockLock(ivars->ringLock);
    for (uint32_t i = 0; i < ioCompletionCount && i < kDisplayRingDepth; i++) {
        uint32_t slot = (ioCompletionIndex + i) % kDisplayRingDepth;
        doneSeq[doneCount]    = ivars->slotSeq[slot];
        doneStatus[doneCount] = statusArray[i];
        doneCount++;

        if (ivars->inflight > 0) {
            ivars->inflight--;
        }
        ivars->tail = (ivars->tail + 1) % kDisplayRingDepth;
        (void)actualByteCountArray[i];
    }
    client = ivars->userClient;
    IOLockUnlock(ivars->ringLock);

    if (client != nullptr) {
        for (uint32_t i = 0; i < doneCount; i++) {
            client->DeliverDisplayDone(doneSeq[i], doneStatus[i]);
        }
    }
}

kern_return_t MaschineMk3DisplayTransport::SubmitBulkOut(const void * bytes, uint32_t length, uint32_t seq)
{
    if (bytes == nullptr || length == 0 || length > kDisplaySlotSize) {
        return kIOReturnBadArgument;
    }
    if (ivars->bulkOutPipe == nullptr || ivars->bundledAction == nullptr) {
        return kIOReturnNotReady;
    }

    uint32_t slot;
    IOLockLock(ivars->ringLock);
    if (ivars->inflight >= kDisplayRingDepth) {
        IOLockUnlock(ivars->ringLock);
        return kIOReturnNoResources;
    }
    slot = ivars->head;
    ivars->head = (ivars->head + 1) % kDisplayRingDepth;
    ivars->inflight++;
    ivars->slotSeq[slot] = seq;
    IOLockUnlock(ivars->ringLock);

    IOBufferMemoryDescriptor * buf = ivars->slots[slot];
    if (buf == nullptr) {
        IOLockLock(ivars->ringLock);
        if (ivars->inflight > 0) ivars->inflight--;
        IOLockUnlock(ivars->ringLock);
        return kIOReturnNotReady;
    }
    IOAddressSegment seg = { 0, 0 };
    kern_return_t gr = buf->GetAddressRange(&seg);
    if (gr != kIOReturnSuccess || seg.address == 0) {
        IOLockLock(ivars->ringLock);
        if (ivars->inflight > 0) ivars->inflight--;
        IOLockUnlock(ivars->ringLock);
        return (gr != kIOReturnSuccess) ? gr : kIOReturnNotReady;
    }
    memcpy((void *)(uintptr_t)seg.address, bytes, length);
    buf->SetLength(length);

    uint32_t lengths[kDisplayRingDepth];
    for (uint32_t i = 0; i < kDisplayRingDepth; i++) {
        lengths[i] = (i == slot) ? length : 0;
    }

    uint32_t accepted = 0;
    kern_return_t kr = ivars->bulkOutPipe->AsyncIOBundled(
        slot, 1, &accepted, lengths, kDisplayRingDepth, ivars->bundledAction, 0);

    if (kr != kIOReturnSuccess || accepted != 1) {
        IOLockLock(ivars->ringLock);
        if (ivars->inflight > 0) ivars->inflight--;
        // Rewind head so this slot can be reused; safe because the failed
        // submission consumed no ring space on the controller side.
        ivars->head = slot;
        IOLockUnlock(ivars->ringLock);
        if (kr == kIOReturnSuccess) {
            kr = kIOReturnNoResources;
        }
    }
    return kr;
}

kern_return_t MaschineMk3DisplayTransport::AbortBulkOut()
{
    if (ivars->bulkOutPipe == nullptr) {
        return kIOReturnNotReady;
    }
    return ivars->bulkOutPipe->Abort(0, kIOReturnAborted, nullptr);
}

kern_return_t MaschineMk3DisplayTransport::AttachClient(MaschineMk3UserClient * client)
{
    IOLockLock(ivars->ringLock);
    ivars->userClient = client;
    IOLockUnlock(ivars->ringLock);
    return kIOReturnSuccess;
}

void MaschineMk3DisplayTransport::DetachClient(MaschineMk3UserClient * client)
{
    IOLockLock(ivars->ringLock);
    if (ivars->userClient == client) {
        ivars->userClient = nullptr;
    }
    IOLockUnlock(ivars->ringLock);
}

uint8_t MaschineMk3DisplayTransport::InterfaceNumber() const { return ivars->interfaceNumber; }
uint8_t MaschineMk3DisplayTransport::BulkOutEndpoint() const { return MASCHINE_BULK_OUT_EP; }
