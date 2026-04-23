//
// MaschineMk3UserClient.cpp
//
// ExternalMethod dispatch + async-push plumbing. Each opened IOUserClient
// connection is bound to exactly one transport (HID or display) — Rust opens
// two connections to cover both, as specified in A1 §2.3.
//

#include <string.h>
#include <os/log.h>
#include <DriverKit/IOUserServer.h>
#include <DriverKit/IOLib.h>
#include <DriverKit/OSData.h>
#include <DriverKit/OSAction.h>
#include <DriverKit/IOUserClient.h>
#include <DriverKit/IOMemoryDescriptor.h>
#include <DriverKit/IOMemoryMap.h>
#include <DriverKit/IOReturn.h>

#include "MaschineIPC.h"
#include "MaschineMk3UserClient.h"
#include "MaschineMk3HidTransport.h"
#include "MaschineMk3DisplayTransport.h"

struct MaschineMk3UserClient_IVars {
    MaschineMk3HidTransport     * hidTransport     = nullptr;
    MaschineMk3DisplayTransport * displayTransport = nullptr;
    OSAction                    * hidInCompletion     = nullptr;
    OSAction                    * displayCompletion   = nullptr;
};

// --------------------------- lifecycle ---------------------------

bool MaschineMk3UserClient::init()
{
    if (!super::init()) {
        return false;
    }
    ivars = IONewZero(MaschineMk3UserClient_IVars, 1);
    return ivars != nullptr;
}

void MaschineMk3UserClient::free()
{
    if (ivars != nullptr) {
        OSSafeReleaseNULL(ivars->hidInCompletion);
        OSSafeReleaseNULL(ivars->displayCompletion);
    }
    IOSafeDeleteNULL(ivars, MaschineMk3UserClient_IVars, 1);
    super::free();
}

kern_return_t IMPL(MaschineMk3UserClient, Start)
{
    kern_return_t ret = Start(provider, SUPERDISPATCH);
    if (ret != kIOReturnSuccess) {
        return ret;
    }

    ivars->hidTransport     = OSDynamicCast(MaschineMk3HidTransport, provider);
    ivars->displayTransport = OSDynamicCast(MaschineMk3DisplayTransport, provider);

    if (ivars->hidTransport != nullptr) {
        ivars->hidTransport->AttachClient(this);
        os_log(OS_LOG_DEFAULT, "MaschineMk3UserClient::Start — bound to HID transport");
    } else if (ivars->displayTransport != nullptr) {
        ivars->displayTransport->AttachClient(this);
        os_log(OS_LOG_DEFAULT, "MaschineMk3UserClient::Start — bound to display transport");
    } else {
        os_log(OS_LOG_DEFAULT, "MaschineMk3UserClient::Start — unknown provider class");
        Stop(provider, SUPERDISPATCH);
        return kIOReturnUnsupported;
    }
    return kIOReturnSuccess;
}

kern_return_t IMPL(MaschineMk3UserClient, Stop)
{
    if (ivars->hidTransport != nullptr) {
        ivars->hidTransport->DetachClient(this);
        ivars->hidTransport = nullptr;
    }
    if (ivars->displayTransport != nullptr) {
        ivars->displayTransport->DetachClient(this);
        ivars->displayTransport = nullptr;
    }
    OSSafeReleaseNULL(ivars->hidInCompletion);
    OSSafeReleaseNULL(ivars->displayCompletion);
    os_log(OS_LOG_DEFAULT, "MaschineMk3UserClient::Stop — user-client closing");
    return Stop(provider, SUPERDISPATCH);
}

// ------------- ExternalMethod dispatch handlers -------------

kern_return_t MaschineMk3UserClient::DispatchOpen(OSObject * target, void * /*reference*/,
                                                  IOUserClientMethodArguments * args)
{
    MaschineMk3UserClient * self = OSDynamicCast(MaschineMk3UserClient, target);
    if (self == nullptr || args->structureInput == nullptr) {
        return kIOReturnBadArgument;
    }
    const uint8_t * inBytes = (const uint8_t *)args->structureInput->getBytesNoCopy();
    if (inBytes == nullptr || args->structureInput->getLength() < sizeof(MaschineOpenIn)) {
        return kIOReturnBadArgument;
    }
    MaschineOpenIn in;
    memcpy(&in, inBytes, sizeof(in));
    if (in.clientVersion != MASCHINE_IPC_VERSION) {
        return kIOReturnUnsupported;
    }

    MaschineOpenOut out;
    memset(&out, 0, sizeof(out));
    out.dextVersion = MASCHINE_IPC_VERSION;
    out.vendorId    = 0x17CCu;
    out.productId   = 0x1600u;
    if (self->ivars->hidTransport != nullptr) {
        out.interfaceNumber = self->ivars->hidTransport->InterfaceNumber();
    } else if (self->ivars->displayTransport != nullptr) {
        out.interfaceNumber = self->ivars->displayTransport->InterfaceNumber();
    }

    args->structureOutput = OSData::withBytes(&out, sizeof(out));
    return (args->structureOutput != nullptr) ? kIOReturnSuccess : kIOReturnNoMemory;
}

kern_return_t MaschineMk3UserClient::DispatchClose(OSObject * /*target*/, void * /*reference*/,
                                                   IOUserClientMethodArguments * /*args*/)
{
    return kIOReturnSuccess;
}

kern_return_t MaschineMk3UserClient::DispatchRegisterHidCallback(OSObject * target, void * /*reference*/,
                                                                 IOUserClientMethodArguments * args)
{
    MaschineMk3UserClient * self = OSDynamicCast(MaschineMk3UserClient, target);
    if (self == nullptr || args->completion == nullptr) {
        return kIOReturnBadArgument;
    }
    if (self->ivars->hidTransport == nullptr) {
        return kIOReturnUnsupported;
    }
    OSSafeReleaseNULL(self->ivars->hidInCompletion);
    args->completion->retain();
    self->ivars->hidInCompletion = args->completion;
    return kIOReturnSuccess;
}

kern_return_t MaschineMk3UserClient::DispatchRegisterDisplayCallback(OSObject * target, void * /*reference*/,
                                                                     IOUserClientMethodArguments * args)
{
    MaschineMk3UserClient * self = OSDynamicCast(MaschineMk3UserClient, target);
    if (self == nullptr || args->completion == nullptr) {
        return kIOReturnBadArgument;
    }
    if (self->ivars->displayTransport == nullptr) {
        return kIOReturnUnsupported;
    }
    OSSafeReleaseNULL(self->ivars->displayCompletion);
    args->completion->retain();
    self->ivars->displayCompletion = args->completion;
    return kIOReturnSuccess;
}

kern_return_t MaschineMk3UserClient::DispatchHidOutReport(OSObject * target, void * /*reference*/,
                                                          IOUserClientMethodArguments * args)
{
    MaschineMk3UserClient * self = OSDynamicCast(MaschineMk3UserClient, target);
    if (self == nullptr || args->structureInput == nullptr) {
        return kIOReturnBadArgument;
    }
    if (self->ivars->hidTransport == nullptr) {
        return kIOReturnUnsupported;
    }

    size_t inLen = args->structureInput->getLength();
    if (inLen < sizeof(uint32_t)) {
        return kIOReturnBadArgument;
    }
    const uint8_t * inBytes = (const uint8_t *)args->structureInput->getBytesNoCopy();
    uint32_t length;
    memcpy(&length, inBytes, sizeof(length));
    if (length == 0 || length > MASCHINE_HID_REPORT_MAX || length + sizeof(uint32_t) > inLen) {
        return kIOReturnBadArgument;
    }
    return self->ivars->hidTransport->SubmitHidOut(inBytes + sizeof(uint32_t), length);
}

kern_return_t MaschineMk3UserClient::DispatchBulkOut(OSObject * target, void * /*reference*/,
                                                     IOUserClientMethodArguments * args)
{
    MaschineMk3UserClient * self = OSDynamicCast(MaschineMk3UserClient, target);
    if (self == nullptr) {
        return kIOReturnBadArgument;
    }
    if (self->ivars->displayTransport == nullptr) {
        return kIOReturnUnsupported;
    }

    const uint8_t * inBytes = nullptr;
    size_t inLen = 0;
    IOMemoryMap * map = nullptr;

    if (args->structureInput != nullptr) {
        inBytes = (const uint8_t *)args->structureInput->getBytesNoCopy();
        inLen   = args->structureInput->getLength();
    } else if (args->structureInputDescriptor != nullptr) {
        kern_return_t mapRet = args->structureInputDescriptor->CreateMapping(
            0, 0, 0, 0, 0, &map);
        if (mapRet != kIOReturnSuccess || map == nullptr) {
            return kIOReturnNoMemory;
        }
        inBytes = (const uint8_t *)(uintptr_t)map->GetAddress();
        inLen   = (size_t)map->GetLength();
    } else {
        return kIOReturnBadArgument;
    }

    kern_return_t ret = kIOReturnBadArgument;
    if (inBytes != nullptr && inLen >= 2 * sizeof(uint32_t)) {
        uint32_t length, seq;
        memcpy(&length, inBytes, sizeof(length));
        memcpy(&seq,    inBytes + sizeof(uint32_t), sizeof(seq));
        if (length > 0 && length <= MASCHINE_BULK_FRAME_MAX &&
            length + 2 * sizeof(uint32_t) <= inLen) {
            ret = self->ivars->displayTransport->SubmitBulkOut(
                inBytes + 2 * sizeof(uint32_t), length, seq);
        }
    }

    OSSafeReleaseNULL(map);
    return ret;
}

kern_return_t MaschineMk3UserClient::DispatchDeviceState(OSObject * target, void * /*reference*/,
                                                         IOUserClientMethodArguments * args)
{
    MaschineMk3UserClient * self = OSDynamicCast(MaschineMk3UserClient, target);
    if (self == nullptr) {
        return kIOReturnBadArgument;
    }

    MaschineDeviceState out;
    memset(&out, 0, sizeof(out));
    out.vendorId[0]  = 0xCC; out.vendorId[1]  = 0x17;
    out.productId[0] = 0x00; out.productId[1] = 0x16;
    out.inMaxPacket   = MASCHINE_HID_IN_PACKET_MAX;
    out.outMaxPacket  = MASCHINE_HID_REPORT_MAX;
    out.bulkMaxPacket = 512;

    if (self->ivars->hidTransport != nullptr) {
        out.bInterfaceNumber = self->ivars->hidTransport->InterfaceNumber();
        out.epInAddr  = self->ivars->hidTransport->HidInEndpoint();
        out.epOutAddr = self->ivars->hidTransport->HidOutEndpoint();
    }
    if (self->ivars->displayTransport != nullptr) {
        if (self->ivars->hidTransport == nullptr) {
            out.bInterfaceNumber = self->ivars->displayTransport->InterfaceNumber();
        }
        out.epBulkAddr = self->ivars->displayTransport->BulkOutEndpoint();
    }

    args->structureOutput = OSData::withBytes(&out, sizeof(out));
    return (args->structureOutput != nullptr) ? kIOReturnSuccess : kIOReturnNoMemory;
}

kern_return_t MaschineMk3UserClient::DispatchAbort(OSObject * target, void * /*reference*/,
                                                   IOUserClientMethodArguments * args)
{
    MaschineMk3UserClient * self = OSDynamicCast(MaschineMk3UserClient, target);
    if (self == nullptr || args->scalarInput == nullptr || args->scalarInputCount < 1) {
        return kIOReturnBadArgument;
    }
    uint64_t mode = args->scalarInput[0];
    kern_return_t ret = kIOReturnSuccess;
    if ((mode == MASCHINE_ABORT_ALL || mode == MASCHINE_ABORT_HID_OUT) && self->ivars->hidTransport != nullptr) {
        ret = self->ivars->hidTransport->AbortHidOut();
    }
    if ((mode == MASCHINE_ABORT_ALL || mode == MASCHINE_ABORT_DISPLAY) && self->ivars->displayTransport != nullptr) {
        kern_return_t r2 = self->ivars->displayTransport->AbortBulkOut();
        if (ret == kIOReturnSuccess) ret = r2;
    }
    return ret;
}

static const IOUserClientMethodDispatch gMethods[kMaschineSelectorCount] = {
    [kSel_Open] = {
        .function                 = (IOUserClientMethodFunction)&MaschineMk3UserClient::DispatchOpen,
        .checkCompletionExists    = false,
        .checkScalarInputCount    = 0,
        .checkStructureInputSize  = sizeof(MaschineOpenIn),
        .checkScalarOutputCount   = 0,
        .checkStructureOutputSize = sizeof(MaschineOpenOut),
    },
    [kSel_Close] = {
        .function                 = (IOUserClientMethodFunction)&MaschineMk3UserClient::DispatchClose,
        .checkCompletionExists    = false,
        .checkScalarInputCount    = 0,
        .checkStructureInputSize  = 0,
        .checkScalarOutputCount   = 0,
        .checkStructureOutputSize = 0,
    },
    [kSel_RegisterHidCallback] = {
        .function                 = (IOUserClientMethodFunction)&MaschineMk3UserClient::DispatchRegisterHidCallback,
        .checkCompletionExists    = true,
        .checkScalarInputCount    = 0,
        .checkStructureInputSize  = 0,
        .checkScalarOutputCount   = 0,
        .checkStructureOutputSize = 0,
    },
    [kSel_RegisterDisplayCallback] = {
        .function                 = (IOUserClientMethodFunction)&MaschineMk3UserClient::DispatchRegisterDisplayCallback,
        .checkCompletionExists    = true,
        .checkScalarInputCount    = 0,
        .checkStructureInputSize  = 0,
        .checkScalarOutputCount   = 0,
        .checkStructureOutputSize = 0,
    },
    [kSel_HidOutReport] = {
        .function                 = (IOUserClientMethodFunction)&MaschineMk3UserClient::DispatchHidOutReport,
        .checkCompletionExists    = false,
        .checkScalarInputCount    = 0,
        .checkStructureInputSize  = kIOUserClientVariableStructureSize,
        .checkScalarOutputCount   = 0,
        .checkStructureOutputSize = 0,
    },
    [kSel_BulkOut] = {
        .function                 = (IOUserClientMethodFunction)&MaschineMk3UserClient::DispatchBulkOut,
        .checkCompletionExists    = false,
        .checkScalarInputCount    = 0,
        .checkStructureInputSize  = kIOUserClientVariableStructureSize,
        .checkScalarOutputCount   = 0,
        .checkStructureOutputSize = 0,
    },
    [kSel_DeviceState] = {
        .function                 = (IOUserClientMethodFunction)&MaschineMk3UserClient::DispatchDeviceState,
        .checkCompletionExists    = false,
        .checkScalarInputCount    = 0,
        .checkStructureInputSize  = 0,
        .checkScalarOutputCount   = 0,
        .checkStructureOutputSize = sizeof(MaschineDeviceState),
    },
    [kSel_Abort] = {
        .function                 = (IOUserClientMethodFunction)&MaschineMk3UserClient::DispatchAbort,
        .checkCompletionExists    = false,
        .checkScalarInputCount    = 1,
        .checkStructureInputSize  = 0,
        .checkScalarOutputCount   = 0,
        .checkStructureOutputSize = 0,
    },
};

kern_return_t MaschineMk3UserClient::ExternalMethod(
    uint64_t selector,
    IOUserClientMethodArguments * arguments,
    const IOUserClientMethodDispatch * dispatch,
    OSObject * target,
    void * reference)
{
    (void)dispatch;
    if (selector >= (uint64_t)kMaschineSelectorCount) {
        return kIOReturnBadArgument;
    }
    return super::ExternalMethod(selector, arguments, &gMethods[selector],
                                 target ? target : this, reference);
}

// -------------- Async push from transport classes ---------------

void MaschineMk3UserClient::DeliverHidIn(const uint8_t * bytes, uint32_t length,
                                         uint32_t seq, uint64_t timestamp)
{
    if (ivars == nullptr || ivars->hidInCompletion == nullptr || bytes == nullptr) {
        return;
    }
    uint32_t clamped = (length > 64u) ? 64u : length;

    MaschineHidInEvent ev;
    memset(&ev, 0, sizeof(ev));
    ev.length    = clamped;
    ev.seq       = seq;
    ev.timestamp = timestamp;
    memcpy(ev.data, bytes, clamped);

    uint64_t scalars[kIOUserClientAsyncArgumentsCountMax];
    memset(scalars, 0, sizeof(scalars));
    static_assert(sizeof(MaschineHidInEvent) == 80, "MaschineHidInEvent must be 80 bytes");
    memcpy(scalars, &ev, sizeof(ev));

    AsyncCompletion(ivars->hidInCompletion, kIOReturnSuccess, scalars, 10);
}

void MaschineMk3UserClient::DeliverDisplayDone(uint32_t seq, kern_return_t status)
{
    if (ivars == nullptr || ivars->displayCompletion == nullptr) {
        return;
    }
    uint64_t scalars[kIOUserClientAsyncArgumentsCountMax];
    memset(scalars, 0, sizeof(scalars));
    scalars[0] = (uint64_t)seq;
    scalars[1] = (uint64_t)(uint32_t)status;
    AsyncCompletion(ivars->displayCompletion, kIOReturnSuccess, scalars, 2);
}
