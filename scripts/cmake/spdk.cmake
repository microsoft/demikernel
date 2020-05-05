# Copyright (c) Microsoft Corporation.
# Licensed under the MIT license.

if(NOT SPDK_DOT_CMAKE_INCLUDED)
set(SPDK_DOT_CMAKE_INCLUDED YES)

include(ExternalProject)
include(dpdk)

# SPDK
set(SPDK_SOURCE_DIR ${CMAKE_CURRENT_SOURCE_DIR}/submodules/spdk)
set(SPDK_BINARY_DIR ${CMAKE_CURRENT_BINARY_DIR}/ExternalProject/spdk)

ExternalProject_Add(spdk
  PREFIX ${SPDK_BINARY_DIR}
  SOURCE_DIR ${SPDK_SOURCE_DIR}
  CONFIGURE_COMMAND cd ${SPDK_SOURCE_DIR} && ./configure --with-dpdk=${DPDK_SOURCE_DIR}/x86_64-native-linuxapp-gcc
  BUILD_COMMAND make -C ${SPDK_SOURCE_DIR}
  INSTALL_COMMAND echo "No install command for target `spdk`."
)

add_dependencies(spdk dpdk)
function(target_add_spdk TARGET)
  target_add_dpdk(${TARGET})
endfunction(target_add_spdk)

endif(NOT SPDK_DOT_CMAKE_INCLUDED)
