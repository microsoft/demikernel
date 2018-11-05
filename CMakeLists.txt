# Zeus libOS code

cmake_minimum_required (VERSION 3.5.1)
project (Zeus C CXX)

# ----------------------------------------------------
# Additional CMake modules
# ----------------------------------------------------

# todo: `ExternalProject` seems more geared towards other CMake projects.
# `add_custom_command()` seems like a better choice for what we want.
include(ExternalProject)
set(CMAKE_MODULE_PATH ${CMAKE_MODULE_PATH}
  "${CMAKE_SOURCE_DIR}/cmake/Modules/")

# default CFLAGS
set(COMMON_CFLAGS "-g -mssse3 -fstack-protector -fPIC -Wall -Wextra -Wno-uninitialized -Wno-unused-parameter -Wl,-zdefs")
set(CMAKE_C_FLAGS "${CMAKE_C_FLAGS} ${COMMON_CFLAGS}")
set(CMAKE_CXX_FLAGS "${CMAKE_CXX_FLAGS}  ${COMMON_CFLAGS} -std=c++11 -D__STDC_LIMIT_MACROS")

# dpdk
set(DPDK_TARGET x86_64-native-linuxapp-gcc CACHE STRING "The DPDK Target")
# note: we have no choice but to use a customized version of dpdk provided
# by mtcp.
set(DPDK_SOURCE_DIR ${CMAKE_CURRENT_SOURCE_DIR}/submodules/mtcp/dpdk-17.08)
set(DPDK_INSTALL_DIR ${CMAKE_CURRENT_BINARY_DIR}/submodules/mtcp/dpdk)
set(DPDK_INCLUDE_DIR ${DPDK_INSTALL_DIR}/include/dpdk)
set(DPDK_LIB_DIR ${DPDK_INSTALL_DIR}/lib/dpdk)
ExternalProject_Add(dpdk
  PREFIX ${DPDK_INSTALL_DIR}
  SOURCE_DIR ${DPDK_SOURCE_DIR}
  CONFIGURE_COMMAND cd ${DPDK_SOURCE_DIR} && make config T=${DPDK_TARGET}
  BUILD_COMMAND cd ${DPDK_SOURCE_DIR} && make T=${DPDK_TARGET}
  INSTALL_COMMAND cd ${DPDK_SOURCE_DIR} && make install T=${DPDK_TARGET} DESTDIR=${DPDK_INSTALL_DIR}
)

# mtcp
set(MTCP_SOURCE_DIR ${CMAKE_CURRENT_SOURCE_DIR}/submodules/mtcp)
set(MTCP_INSTALL_DIR ${CMAKE_CURRENT_BINARY_DIR}/submodules/mtcp)
set(LIBMTCP ${MTCP_SOURCE_DIR}/mtcp/lib/libmtcp.a)
ExternalProject_Add(mtcp
  DEPENDS dpdk
  PREFIX ${MTCP_INSTALL_DIR}
  SOURCE_DIR ${MTCP_SOURCE_DIR}
  CONFIGURE_COMMAND cd ${MTCP_SOURCE_DIR} && ./configure --with-dpdk-lib=${DPDK_SOURCE_DIR}/${DPDK_TARGET} --prefix=${MTCP_INSTALL_DIR} CFLAGS=-I${CMAKE_CURRENT_SOURCE_DIR}
  BUILD_COMMAND cd ${MTCP_SOURCE_DIR} && make
  INSTALL_COMMAND echo 'No `install` target has been implemented for MTCP'
)
function(target_add_mtcp TARGET)
  target_link_libraries(${TARGET} ${LIBMTCP})
  target_include_directories(${TARGET} PUBLIC ${MTCP_SOURCE_DIR}/mtcp/include)
endfunction(target_add_mtcp)

# spdk
set(SPDK_SOURCE_DIR ${CMAKE_CURRENT_SOURCE_DIR}/submodules/spdk)
set(SPDK_INSTALL_DIR ${CMAKE_CURRENT_BINARY_DIR}/submodules/spdk)
ExternalProject_Add(spdk
  DEPENDS dpdk
  PREFIX ${SPDK_INSTALL_DIR}
  SOURCE_DIR ${SPDK_SOURCE_DIR}
  CONFIGURE_COMMAND cd ${SPDK_SOURCE_DIR} && ./configure --prefix=${SPDK_INSTALL_DIR}
  BUILD_COMMAND cd ${SPDK_SOURCE_DIR} && make
  INSTALL_COMMAND cd ${SPDK_SOURCE_DIR} && make install
)
function(target_add_spdk TARGET)
  #target_link_libraries(${TARGET} ${LIBSPDK})
  target_include_directories(${TARGET} PUBLIC ${SPDK_SOURCE_DIR}/include)
endfunction(target_add_spdk)

# todo: can these be compiled into a common library?
set(ZEUS_SOURCE_DIR ${CMAKE_CURRENT_SOURCE_DIR}/libos)
file(GLOB ZEUS_COMMON_SOURCES "${ZEUS_SOURCE_DIR}/common/*.cc")
include_directories(
  ${CMAKE_CURRENT_SOURCE_DIR}/include
  ${CMAKE_CURRENT_SOURCE_DIR}
)

# hoard
set(HOARD_SOURCE_DIR ${ZEUS_SOURCE_DIR}/common/mem)
set(LIBHOARD ${HOARD_SOURCE_DIR}/libhoard.so)
add_custom_command(
  OUTPUT ${LIBHOARD}
  WORKING_DIRECTORY ${HOARD_SOURCE_DIR}
  COMMAND make
)
function(target_add_hoard TARGET)
  target_link_libraries(${TARGET} ${LIBHOARD})
  target_include_directories(${TARGET} PUBLIC
    ${HOARD_SOURCE_DIR}/include
    ${HOARD_SOURCE_DIR}/include/hoard
    ${HOARD_SOURCE_DIR}/include/superblocks
    ${HOARD_SOURCE_DIR}/include/util
    ${HOARD_SOURCE_DIR}/include/zeus
    ${HOARD_SOURCE_DIR}/Heap-Layers
  )
endfunction(target_add_hoard)

# POSIX libos target
file(GLOB ZEUS_POSIX_SOURCES "${ZEUS_SOURCE_DIR}/libposix/*.cc")
add_library(zeus_posix SHARED ${ZEUS_COMMON_SOURCES} ${ZEUS_POSIX_SOURCES})

# LWIP libos target
file(GLOB ZEUS_LIBLWIP_SOURCES ${ZEUS_SOURCE_DIR}/liblwip/*.cc)
add_library(zeus_lwip STATIC ${ZEUS_LIBLWIP_SOURCES} ${ZEUS_COMMON_SOURCES})
target_add_hoard(zeus_lwip)
target_include_directories(zeus_lwip PUBLIC ${ZEUS_SOURCE_DIR}/liblwip ${DPDK_INCLUDE_DIR} )
# todo: these options should not be passed directly to
# `target_link_libraries()`. we should find another way.
target_link_libraries(zeus_lwip "-Wl,--whole-archive -lnuma -lpthread -ldl -lrte_mbuf  -lrte_eal -lrte_mempool -lrte_mempool_ring -lrte_ring -lrte_ethdev -lrte_kvargs -lrte_pmd_mlx5 -libverbs -Wl,--no-whole-archive")
add_dependencies(zeus_lwip dpdk)

# MTCP libos target
file(GLOB ZEUS_MTCP_SOURCES "${ZEUS_SOURCE_DIR}/libmtcp/*.cc")
add_library(zeus_mtcp STATIC ${ZEUS_COMMON_SOURCES} ${ZEUS_MTCP_SOURCES})
target_add_hoard(zeus_mtcp)
target_add_mtcp(zeus_mtcp)
add_dependencies(zeus_mtcp mtcp)

# RDMA libos target
file(GLOB ZEUS_RDMA_SOURCES "${ZEUS_SOURCE_DIR}/librdma/*.cc")
add_library(zeus_rdma SHARED ${ZEUS_COMMON_SOURCES} ${ZEUS_RDMA_SOURCES})
target_add_hoard(zeus_rdma)
target_link_libraries(zeus_rdma rdmacm ibverbs)

# spdk libos target
file(GLOB ZEUS_SPDK_LIB "${ZEUS_SOURCE_DIR}/libspdk/*.cc")
add_library(zeus_spdk STATIC ${ZEUS_COMMON_SOURCES} ${ZEUS_SPDK_LIB} ${ZEUS_POSIX_SOURCES})
target_add_hoard(zeus_spdk)
target_add_spdk(zeus_spdk)
add_dependencies(zeus_spdk spdk)

#############################################
# applications
# Tapir
set(TAPIR_APP_DIR ${CMAKE_CURRENT_SOURCE_DIR}/apps/tapir)
add_custom_target(tapir
                  COMMAND ZEUS_SRC_DIR=${CMAKE_CURRENT_SOURCE_DIR} make
                  WORKING_DIRECTORY ${TAPIR_APP_DIR})
add_dependencies(tapir zeus_posix)

add_custom_target(tapir-clean
                  COMMAND make clean
                  WORKING_DIRECTORY ${TAPIR_APP_DIR})


# Redis
set(REDIS_APP_DIR ${CMAKE_CURRENT_SOURCE_DIR}/apps/redis)
add_custom_target(redis
                  COMMAND ZEUS_SRC_DIR=${CMAKE_CURRENT_SOURCE_DIR} make
                  WORKING_DIRECTORY ${REDIS_APP_DIR})

#add_subdirectory(apps/echo)
