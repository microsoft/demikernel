# Copyright (c) Microsoft Corporation.
# Licensed under the MIT license.

configure_file(${CMAKE_CURRENT_SOURCE_DIR}/config.yaml ${CMAKE_CURRENT_BINARY_DIR}/config.yaml COPYONLY)

add_subdirectory(latency)
add_subdirectory(libos)
add_subdirectory(apps)
