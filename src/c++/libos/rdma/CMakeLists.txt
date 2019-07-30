# Copyright (c) Microsoft Corporation.
# Licensed under the MIT license.

# RDMA libos target
file(GLOB ZEUS_RDMA_SOURCES "${CMAKE_CURRENT_SOURCE_DIR}/*.cc")
# note: the libos library has to be a shared object in order to
# support the fact that we develop in C++ but need to also support
# applications written in C.
add_library(dmtr-libos-rdma SHARED ${ZEUS_RDMA_SOURCES})
target_add_hoard(dmtr-libos-rdma hoard-rdma)

# the `--whole-archive` option is needed to ensure that symbols such as
# `zeus_queue` get exported from the resulting shared object. object
# files are normally culled if none of their symbols are referenced.
target_link_libraries(dmtr-libos-rdma "-Wl,--whole-archive" dmtr-libos-common "-Wl,--no-whole-archive")

target_link_libraries(dmtr-libos-rdma dmtr-libos-rdmacm-common rdmacm ibverbs boost_context boost_coroutine boost_system boost_chrono dmtr-latency)
