# Copyright (c) Microsoft Corporation.
# Licensed under the MIT license.

file(GLOB LIBOS_DPDK_CATNIP_SOURCES ${CMAKE_CURRENT_SOURCE_DIR}/*.cc)
add_library(dmtr-libos-dpdk-catnip SHARED ${LIBOS_DPDK_CATNIP_SOURCES})
target_add_hoard(dmtr-libos-dpdk-catnip hoard-vanilla)
target_add_dpdk(dmtr-libos-dpdk-catnip)

add_dependencies(dmtr-libos-dpdk-catnip catnip)

target_include_directories(dmtr-libos-dpdk-catnip PUBLIC ${DPDK_INCLUDE_DIR} ${CMAKE_SOURCE_DIR}/src/rust/catnip/include)

target_link_libraries(dmtr-libos-dpdk-catnip Threads::Threads yaml-cpp boost_program_options ${CMAKE_BINARY_DIR}/src/rust/catnip/lib/libcatnip.a)

# the `--whole-archive` option is needed to ensure that symbols such as
# `zeus_queue` get exported from the resulting shared object. object
# files are normally culled if none of their symbols are referenced.
target_link_libraries(dmtr-libos-dpdk-catnip "-Wl,--whole-archive" dmtr-libos-common "-Wl,--no-whole-archive")

# todo: these options should not be passed directly to
# `target_link_libraries()`. we should use `find_library()` instead.
target_link_libraries(dmtr-libos-dpdk-catnip "-Wl,--whole-archive -lnuma -Wl,--no-whole-archive")

# `boost_context` and `boost_coroutine` must follow `dmtr-libos-common`
target_link_libraries(dmtr-libos-dpdk-catnip boost_context boost_coroutine dmtr-latency)

