# Copyright (c) Microsoft Corporation.
# Licensed under the MIT license.

if(NOT DPDK_DOT_CMAKE_INCLUDED)
set(DPDK_DOT_CMAKE_INCLUDED YES)

include(ExternalProject)
include(list)
include(azure)

option(DPDK_MLX4_SUPPORT "Include DPDK support for Mellanox ConnectX-3 and Mellanox ConnectX-3 Pro 10/40 Gbps adapters" OFF)
option(DPDK_MLX5_SUPPORT "Include DPDK support for Mellanox ConnectX-4, Mellanox ConnectX-4 Lx, Mellanox ConnectX-5, Mellanox ConnectX-6 and Mellanox BlueField families of 10/25/40/50/100/200 Gb/s adapters" OFF)
set(DPDK_TARGET x86_64-native-linuxapp-gcc CACHE STRING "The DPDK Target")

# DPDK
set(DPDK_SOURCE_DIR ${CMAKE_SOURCE_DIR}/submodules/dpdk)
set(DPDK_BINARY_DIR ${CMAKE_BINARY_DIR}/ExternalProject/dpdk)
set(DPDK_INSTALL_DIR ${DPDK_BINARY_DIR})
set(DPDK_INCLUDE_DIR ${DPDK_INSTALL_DIR}/include ${DPDK_INSTALL_DIR}/include/dpdk)
set(DPDK_LIB_DIR ${DPDK_INSTALL_DIR}/lib)

# we hacked the DPDK build to divulge the flags it generated for
# compilation and linking-- a technique borrowed from mTCP.
set(DPDK_CFLAGS_FILE ${DPDK_SOURCE_DIR}/${DPDK_TARGET}/include/cflags.txt)
set(DPDK_LDFLAGS_FILE ${DPDK_SOURCE_DIR}/${DPDK_TARGET}/lib/ldflags.txt)

if(CMAKE_BUILD_TYPE MATCHES "Rel")
    set(DPDK_EXTRA_CFLAGS "-fPIC -O3")
else(CMAKE_BUILD_TYPE MATCHES "Rel")
    set(DPDK_EXTRA_CFLAGS "-fPIC -O0 -g3")
endif(CMAKE_BUILD_TYPE MATCHES "Rel")

# warning: the same build flags have to be passed to both the build command
# and the install command (or `EXTRA_CFLAGS` could be wiped out during
# install).
ExternalProject_Add(dpdk
    PREFIX ${DPDK_BINARY_DIR}
    SOURCE_DIR ${DPDK_SOURCE_DIR}
    CONFIGURE_COMMAND make -C ${DPDK_SOURCE_DIR} config  T=${DPDK_TARGET}
    BUILD_COMMAND make -C ${DPDK_SOURCE_DIR} T=${DPDK_TARGET} DESTDIR=${DPDK_INSTALL_DIR} EXTRA_CFLAGS=${DPDK_EXTRA_CFLAGS} V=1
    INSTALL_COMMAND make -C ${DPDK_SOURCE_DIR} install T=${DPDK_TARGET}  DESTDIR=${DPDK_INSTALL_DIR} EXTRA_CFLAGS=${DPDK_EXTRA_CFLAGS} V=1
)

# configure DPDK options.
if(DPDK_MLX4_SUPPORT OR AZURE_SUPPORT)
    set(DPDK_CONFIG_RTE_LIBRTR_MLX4_PMD y)
else(DPDK_MLX4_SUPPORT OR AZURE_SUPPORT)
    set(DPDK_CONFIG_RTE_LIBRTR_MLX4_PMD n)
endif(DPDK_MLX4_SUPPORT OR AZURE_SUPPORT)
if(DPDK_MLX5_SUPPORT OR AZURE_SUPPORT)
    set(DPDK_CONFIG_RTE_LIBRTR_MLX5_PMD y)
else(DPDK_MLX5_SUPPORT OR AZURE_SUPPORT)
    set(DPDK_CONFIG_RTE_LIBRTR_MLX5_PMD n)
endif(DPDK_MLX5_SUPPORT OR AZURE_SUPPORT)
if(AZURE_SUPPORT)
    set(DPDK_CONFIG_RTE_LIBRTE_VDEV_NETVSC_PMD y)
else(AZURE_SUPPORT)
    set(DPDK_CONFIG_RTE_LIBRTE_VDEV_NETVSC_PMD n)
endif(AZURE_SUPPORT)
set(DPDK_CONFIG_COMMON_BASE ${DPDK_SOURCE_DIR}/config/common_base)
configure_file(${DPDK_CONFIG_COMMON_BASE}.in ${DPDK_CONFIG_COMMON_BASE})

function(target_add_dpdk TARGET)
    target_include_directories(${TARGET} PUBLIC ${DPDK_INCLUDE_DIR})
    set_target_properties(${TARGET} PROPERTIES
        COMPILE_FLAGS @${DPDK_CFLAGS_FILE}
        LINK_FLAGS @${DPDK_LDFLAGS_FILE}
    )
    add_dependencies(${TARGET} dpdk)
endfunction(target_add_dpdk)

endif(NOT DPDK_DOT_CMAKE_INCLUDED)
