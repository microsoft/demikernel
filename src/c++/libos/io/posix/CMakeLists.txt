# Copyright (c) Microsoft Corporation.
# Licensed under the MIT license.

# POSIX libos target
file(GLOB ZEUS_POSIX_SOURCES "${CMAKE_CURRENT_SOURCE_DIR}/*.cc")
# note: the libos library has to be a shared object in order to
# support the fact that we develop in C++ but need to also support
# applications written in C.
add_library(dmtr-libos-posix SHARED ${ZEUS_POSIX_SOURCES})

if(CMAKE_BUILD_TYPE MATCHES "Rel")
    target_add_hoard(dmtr-libos-posix hoard-vanilla)
endif(CMAKE_BUILD_TYPE MATCHES "Rel")

# the `--whole-archive` option is needed to ensure that symbols such as
# `dmtr_queue` get exported from the resulting shared object. object
# files are normally culled if none of their symbols are referenced.
# todo: is this still necessary?
target_link_libraries(dmtr-libos-posix "-Wl,--whole-archive" dmtr-libos-common "-Wl,--no-whole-archive")

target_link_libraries(dmtr-libos-posix boost_context dmtr-latency)
