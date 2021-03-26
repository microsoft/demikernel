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
  ${SPDK_SOURCE_DIR}/build/lib/libspdk_util.a)
  
set(SPDK_LIBS2
  ${SPDK_SOURCE_DIR}/build/lib/libspdk_env_dpdk.a)
  
ExternalProject_Add(spdk
  PREFIX ${SPDK_BINARY_DIR}
  SOURCE_DIR ${SPDK_SOURCE_DIR}
  CONFIGURE_COMMAND cd ${SPDK_SOURCE_DIR} && ./configure --with-dpdk=${DPDK_INSTALL_DIR} --disable-examples --disable-tests
  BUILD_COMMAND make -C ${SPDK_SOURCE_DIR}
  INSTALL_COMMAND echo "No install command for target `spdk`.")  
add_dependencies(spdk dpdk)

function(target_add_spdk TARGET)
  add_dependencies(${TARGET} spdk)
  target_link_libraries(${TARGET} "-Wl,--whole-archive" ${SPDK_LIBS})
  target_link_libraries(${TARGET} "-Wl,--no-whole-archive" ${SPDK_LIBS2})
  target_include_directories(${TARGET} PUBLIC ${SPDK_SOURCE_DIR}/include)
  target_add_dpdk(${TARGET})
  target_link_libraries(${TARGET} uuid)
endfunction(target_add_spdk)

# SPDK with RDMA dependencies
set(SPDK_RDMA_SOURCE_DIR ${CMAKE_CURRENT_SOURCE_DIR}/submodules/spdk-rdma)
set(SPDK_RDMA_BINARY_DIR ${CMAKE_CURRENT_BINARY_DIR}/ExternalProject/spdk-rdma)

set(SPDK_RDMA_LIBS
  ${SPDK_RDMA_SOURCE_DIR}/build/lib/libspdk_log.a
  ${SPDK_RDMA_SOURCE_DIR}/build/lib/libspdk_nvme.a
  ${SPDK_RDMA_SOURCE_DIR}/build/lib/libspdk_sock.a
  ${SPDK_RDMA_SOURCE_DIR}/build/lib/libspdk_sock_posix.a
  ${SPDK_RDMA_SOURCE_DIR}/build/lib/libspdk_thread.a
  ${SPDK_RDMA_SOURCE_DIR}/build/lib/libspdk_util.a)
  
set(SPDK_RDMA_LIBS2
  ${SPDK_RDMA_SOURCE_DIR}/build/lib/libspdk_env_dpdk.a)
  
ExternalProject_Add(spdk-rdma
  PREFIX ${SPDK_RDMA_BINARY_DIR}
  SOURCE_DIR ${SPDK_RDMA_SOURCE_DIR}
  CONFIGURE_COMMAND cd ${SPDK_RDMA_SOURCE_DIR} && ./configure --with-dpdk=${DPDK_INSTALL_DIR} --with-rdma
  BUILD_COMMAND make -C ${SPDK_RDMA_SOURCE_DIR}
  INSTALL_COMMAND echo "No install command for target `spdk-rdma`.")
add_dependencies(spdk-rdma dpdk)

function(target_add_spdk_rdma TARGET)
    add_dependencies(${TARGET} spdk-rdma)
  target_link_libraries(${TARGET} "-Wl,--whole-archive" ${SPDK_RDMA_LIBS})
  target_link_libraries(${TARGET} "-Wl,--no-whole-archive" ${SPDK_RDMA_LIBS2})
  target_include_directories(${TARGET} PUBLIC ${SPDK_RDMA_SOURCE_DIR}/include)
  target_add_dpdk(${TARGET})
  target_link_libraries(${TARGET} uuid rdmacm ibverbs)
endfunction(target_add_spdk_rdma)

endif(NOT SPDK_DOT_CMAKE_INCLUDED)
