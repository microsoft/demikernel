# Copyright (c) Microsoft Corporation.
# Licensed under the MIT license.

add_subdirectory(${CMAKE_CURRENT_SOURCE_DIR}/common)
add_subdirectory(${CMAKE_CURRENT_SOURCE_DIR}/lwip)
add_subdirectory(${CMAKE_CURRENT_SOURCE_DIR}/posix)
add_subdirectory(${CMAKE_CURRENT_SOURCE_DIR}/rdma)
add_subdirectory(${CMAKE_CURRENT_SOURCE_DIR}/rdmacm-common)
add_subdirectory(${CMAKE_CURRENT_SOURCE_DIR}/spdk-dpdk)
add_subdirectory(${CMAKE_CURRENT_SOURCE_DIR}/spdk-rdma)
add_subdirectory(${CMAKE_CURRENT_SOURCE_DIR}/spdk)
add_subdirectory(${CMAKE_CURRENT_SOURCE_DIR}/base)

add_subdirectory(${CMAKE_CURRENT_SOURCE_DIR}/catnip_libos)
