# Copyright (c) Microsoft Corporation.
# Licensed under the MIT license.

file(GLOB ZEUS_LIBSPDKDPDK_SOURCES ${CMAKE_CURRENT_SOURCE_DIR}/*.cc)
# note: the libos library has to be a shared object in order to
# support the fact that we develop in C++ but need to also support
# applications written in C.
add_library(dmtr-libos-spdk-dpdk SHARED ${ZEUS_LIBSPDKDPDK_SOURCES})
target_add_hoard(dmtr-libos-spdk-dpdk hoard-vanilla)
target_add_spdk(dmtr-libos-spdk-dpdk)
target_include_directories(dmtr-libos-spdk-dpdk PUBLIC ${CMAKE_CURRENT_SOURCE_DIR}/libspdk-dpdk ${DPDK_INCLUDE_DIR} ${SPDK_INCLUDE_DIR} )

target_link_libraries(dmtr-libos-spdk-dpdk Threads::Threads yaml-cpp boost_program_options)

# the `--whole-archive` option is needed to ensure that symbols such as
# `zeus_queue` get exported from the resulting shared object. object
# files are normally culled if none of their symbols are referenced.
target_link_libraries(dmtr-libos-spdk-dpdk "-Wl,--whole-archive" dmtr-libos-common dmtr-libos-lwip "-Wl,--no-whole-archive")

# todo: these options should not be passed directly to
# `target_link_libraries()`. we should use `find_library()` instead.
target_link_libraries(dmtr-libos-spdk-dpdk "-Wl,--whole-archive -lnuma -Wl,--no-whole-archive")

# `boost_context` and `boost_coroutine` must follow `dmtr-libos-common`
target_link_libraries(dmtr-libos-spdk-dpdk boost_context boost_coroutine dmtr-latency)

