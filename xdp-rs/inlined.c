/*
 * Copyright (c) Microsoft Corporation.
 * Licensed under the MIT license.
 */

/*
 * This file contains wrappers for inline functions in <xdpapi.h>.
 */

#include "wrapper.h"

HRESULT
_XdpLoadApi(
    _In_ UINT32 XdpApiVersion,
    _Out_ XDP_LOAD_API_CONTEXT *XdpLoadApiContext,
    _Out_ CONST XDP_API_TABLE **XdpApiTable
    )
{
    return XdpLoadApi(XdpApiVersion, XdpLoadApiContext, XdpApiTable);
}


VOID
_XdpUnloadApi(
    _In_ XDP_LOAD_API_CONTEXT XdpLoadApiContext,
    _In_ CONST XDP_API_TABLE *XdpApiTable
    )
{
    XdpUnloadApi(XdpLoadApiContext, XdpApiTable);
}

VOID
_XskRingInitialize(
    _Out_ XSK_RING *Ring,
    _In_ const XSK_RING_INFO *RingInfo
    )
{
    XskRingInitialize(Ring, RingInfo);
}

UINT32
_XskRingConsumerReserve(
    _In_ XSK_RING *Ring,
    _In_ UINT32 MaxCount,
    _Out_ UINT32 *Index
    )
{
    return XskRingConsumerReserve(Ring, MaxCount, Index);
}

UINT32
_XskRingProducerReserve(
    _In_ XSK_RING *Ring,
    _In_ UINT32 MaxCount,
    _Out_ UINT32 *Index
    )
{
    return XskRingProducerReserve(Ring, MaxCount, Index);
}

VOID
_XskRingConsumerRelease(
    _Inout_ XSK_RING *Ring,
    _In_ UINT32 Count
    )
{
    XskRingConsumerRelease(Ring, Count);
}

VOID
_XskRingProducerSubmit(
    _Inout_ XSK_RING *Ring,
    _In_ UINT32 Count
    )
{
    XskRingProducerSubmit(Ring, Count);
}

VOID *
_XskRingGetElement(
    _In_ const XSK_RING *Ring,
    _In_ UINT32 Index
    ) {
    return XskRingGetElement(Ring, Index);
    }
