# Copyright (c) Microsoft Corporation.
# Licensed under the MIT license.

file(GLOB ZEUS_COMMON_SOURCES "${CMAKE_CURRENT_SOURCE_DIR}/*.c" "${CMAKE_CURRENT_SOURCE_DIR}/*.cc")
add_library(dmtr-libos-io-common STATIC ${ZEUS_COMMON_SOURCES})
target_link_libraries(dmtr-libos-io-common psp-libos-common dmtr-trace dmtr-latency dmtr-time boost_program_options boost_context boost_coroutine boost_system boost_chrono pthread)
