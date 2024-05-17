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
