# Copyright (c) Microsoft Corporation.
# Licensed under the MIT license.

file(GLOB DMTR_TRACE_SOURCES "${CMAKE_CURRENT_SOURCE_DIR}/*.c" "${CMAKE_CURRENT_SOURCE_DIR}/*.cc")
add_library(dmtr-trace STATIC ${DMTR_TRACE_SOURCES})
target_link_libraries(dmtr-trace boost_chrono boost_system)
