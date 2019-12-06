# Copyright (c) Microsoft Corporation.
# Licensed under the MIT license.

file(GLOB ZEUS_COMMON_SOURCES "${CMAKE_CURRENT_SOURCE_DIR}/*.c" "${CMAKE_CURRENT_SOURCE_DIR}/*.cc")
add_library(dmtr-libos-io-common STATIC ${ZEUS_COMMON_SOURCES})
target_link_libraries(dmtr-libos-io-common psp-libos-common dmtr-latency dmtr-time boost_program_options pthread)
