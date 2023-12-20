/*
 * Copyright (c) Microsoft Corporation.
 * Licensed under the MIT license.
 */

#include <rte_build_config.h>

/*
 * work around bug in dpdk headers where the toolchain preprocessor
 * definition used to build dpdk retained and incorrectly directs
 * conditional compilation during application build.
 */
#undef RTE_TOOLCHAIN
#ifdef RTE_TOOLCHAIN_CLANG
#undef RTE_TOOLCHAIN_CLANG
#endif
#ifdef RTE_TOOLCHAIN_GCC
#undef RTE_TOOLCHAIN_GCC
#endif
#ifdef RTE_TOOLCHAIN_MSVC
#undef RTE_TOOLCHAIN_MSVC
#endif
#define RTE_TOOLCHAIN "clang"
#define RTE_TOOLCHAIN_CLANG 1

#include <rte_ethdev.h>
#include <rte_common.h>
#include <rte_cycles.h>
#include <rte_eal.h>
#include <rte_ip.h>
#include <rte_lcore.h>
#include <rte_memcpy.h>
#include <rte_udp.h>
#include <rte_mbuf.h>
