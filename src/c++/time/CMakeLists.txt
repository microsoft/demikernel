# Copyright (c) Microsoft Corporation.
# Licensed under the MIT license.

file(GLOB DMTR_TIME_SOURCES "${CMAKE_CURRENT_SOURCE_DIR}/*.c" "${CMAKE_CURRENT_SOURCE_DIR}/*.cc")
add_library(dmtr-time STATIC ${DMTR_TIME_SOURCES})
target_link_libraries(dmtr-time boost_chrono boost_system)
