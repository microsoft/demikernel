# Copyright (c) Microsoft Corporation.
# Licensed under the MIT license.

file(GLOB ZEUS_COMMON_SOURCES "${CMAKE_CURRENT_SOURCE_DIR}/*.c" "${CMAKE_CURRENT_SOURCE_DIR}/*.cc")
add_library(dmtr-libos-common STATIC ${ZEUS_COMMON_SOURCES})
target_link_libraries(dmtr-libos-common dmtr-latency)
