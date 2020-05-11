# Copyright (c) Microsoft Corporation.
# Licensed under the MIT license.

if(NOT SPDK_DOT_CMAKE_INCLUDED)
set(SPDK_DOT_CMAKE_INCLUDED YES)

include(ExternalProject)
include(dpdk)

# SPDK
set(SPDK_SOURCE_DIR ${CMAKE_CURRENT_SOURCE_DIR}/submodules/spdk)
set(SPDK_BINARY_DIR ${CMAKE_CURRENT_BINARY_DIR}/ExternalProject/spdk)

set(SPDK_LIBS
  ${SPDK_SOURCE_DIR}/build/lib/libspdk_log.a
  ${SPDK_SOURCE_DIR}/build/lib/libspdk_nvme.a
  ${SPDK_SOURCE_DIR}/build/lib/libspdk_sock.a
  ${SPDK_SOURCE_DIR}/build/lib/libspdk_sock_posix.a
  ${SPDK_SOURCE_DIR}/build/lib/libspdk_thread.a
  ${SPDK_SOURCE_DIR}/build/lib/libspdk_util.a
)
set(SPDK_LIBS2
  ${SPDK_SOURCE_DIR}/build/lib/libspdk_env_dpdk.a
  )

ExternalProject_Add(spdk
  PREFIX ${SPDK_BINARY_DIR}
  SOURCE_DIR ${SPDK_SOURCE_DIR}
  CONFIGURE_COMMAND cd ${SPDK_SOURCE_DIR} && ./configure --with-dpdk=${DPDK_BINARY_DIR}
  BUILD_COMMAND make -C ${SPDK_SOURCE_DIR}
  INSTALL_COMMAND echo "No install command for target `spdk`."
)
message("${DPDK_INSTALL_DIR}")
add_dependencies(spdk dpdk)
function(target_add_spdk TARGET)
  target_link_libraries(${TARGET} "-Wl,--whole-archive" ${SPDK_LIBS})
  target_link_libraries(${TARGET} "-Wl,--no-whole-archive" ${SPDK_LIBS2})
#  target_link_libraries(${TARGET} ${DPDK_SOURCE_DIR}/build/lib/librte_pci.a)
  target_include_directories(${TARGET} PUBLIC ${SPDK_SOURCE_DIR}/include)
  target_add_dpdk(${TARGET})
  target_link_libraries(${TARGET} uuid)
endfunction(target_add_spdk)

endif(NOT SPDK_DOT_CMAKE_INCLUDED)
