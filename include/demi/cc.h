// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#ifndef DEMI_CC_H
#define DEMI_CC_H

#ifdef _MSC_VER
#include <sal.h>
#else
#define _In_
#define _In_z_
#define _In_opt_
#define _In_reads_(s)
#define _In_reads_bytes_(b)
#define _Out_
#define _Out_writes_to_(s, c)
#define _Deref_pre_z_
#endif

#if defined(__GNUC__)
// GCC and clang both support __attribute__((nonnull(...)))
// Indices are one-based.
// Note that while clang support _Nonnull and _Nullable, they have different positional syntax
// w.r.t. SAL, so supporting both is complex.
#define ATTR_NONNULL(...) __attribute__((nonnull(__VA_ARGS__)))
#define ATTR_NODISCARD __attribute__((warn_unused_result))
#elif defined(_MSC_VER)
// MSVC uses SAL; NONNULL is supported via _In_/_Out_/etc.
#define ATTR_NONNULL(...)
#define ATTR_NODISCARD _Check_return_
#else
#define ATTR_NONNULL(...)
#define ATTR_NODISCARD _Check_return_
#endif

#endif /* DEMI_CC_H_ */
