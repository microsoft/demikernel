# Copyright (c) Microsoft Corporation.
# Licensed under the MIT license.

file(GLOB DMTR_LATENCY_SOURCES "${CMAKE_CURRENT_SOURCE_DIR}/*.c" "${CMAKE_CURRENT_SOURCE_DIR}/*.cc")
add_library(dmtr-latency STATIC ${DMTR_LATENCY_SOURCES})
target_link_libraries(dmtr-latency boost_chrono boost_system)
