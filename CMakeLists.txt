# Zeus libOS code

cmake_minimum_required (VERSION 3.5.1)
project (Zeus C CXX)

set(ZEUS_DIR ${CMAKE_CURRENT_SOURCE_DIR})

# ----------------------------------------------------
# Additional CMake modules
# ----------------------------------------------------

include(ExternalProject)
set(CMAKE_MODULE_PATH ${CMAKE_MODULE_PATH}
  "${CMAKE_SOURCE_DIR}/cmake/Modules/")

# ----------------------------------------------------
# C Flags
# ----------------------------------------------------

# todo: are these applied to all configurations? we need to split up the options
# between those that help with debugging and those that help with performance.
# todo: is `-O3` safe?
set(CMAKE_C_FLAGS "${CMAKE_C_FLAGS} -g -fstack-protector -Wall \
  -Wno-uninitialized -O3 -DNDEBUG")
set(CMAKE_CXX_FLAGS "${CMAKE_CXX_FLAGS} -g -fstack-protector \
   -std=c++11 -fPIC -Wall -Wl,-zdefs")
   set(CMAKE_LD_FLAGS "${CMAKE_LD_FLAGS}")

# submodules
# ----------
#
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
#
# mtcp
set(MTCP_SOURCE_DIR ${CMAKE_CURRENT_SOURCE_DIR}/submodules/mtcp)
set(MTCP_INSTALL_DIR ${CMAKE_CURRENT_BINARY_DIR}/submodules/mtcp)
ExternalProject_Add(mtcp
  DEPENDS dpdk
  PREFIX ${MTCP_INSTALL_DIR}
  SOURCE_DIR ${MTCP_SOURCE_DIR}
  CONFIGURE_COMMAND cd ${MTCP_SOURCE_DIR} && ./configure --with-dpdk-lib=${DPDK_SOURCE_DIR}/${DPDK_TARGET} --prefix=${MTCP_INSTALL_DIR} CFLAGS=-I${CMAKE_CURRENT_SOURCE_DIR}
  BUILD_COMMAND cd ${MTCP_SOURCE_DIR} && make
  INSTALL_COMMAND echo 'No `install` target has been implemented for MTCP'
)
#
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

# ---------------------------------------------------------
# Include and link directories
# ---------------------------------------------------------

# todo: put these in subdirectories.
# todo: trigger submodule builds from within cmake.
set(SUBMODULES_DIR ${ZEUS_DIR}/submodules)
set(SPDK_DIR ${SUBMODULES_DIR}/spdk CACHE PATH "Path to SPDK")

set(ZEUS_SOURCE_DIR ${ZEUS_DIR}/libos)
include_directories(${ZEUS_DIR})
include_directories(${ZEUS_SOURCE_DIR})

# Hoard
set(HOARD_DIR ${ZEUS_SOURCE_DIR}/common/mem)
include_directories(${HOARD_DIR}/include)
include_directories(${HOARD_DIR}/include/hoard)
include_directories(${HOARD_DIR}/include/superblocks)
include_directories(${HOARD_DIR}/include/util)
include_directories(${HOARD_DIR}/include/zeus)
include_directories(${HOARD_DIR}/Heap-Layers)

# mtcp
include_directories(${MTCP_SOURCE_DIR}/mtcp/include)

# LWIP
include_directories(${DPDK_INCLUDE_DIR})
link_directories(${DPDK_LIB_DIR})

# spdk
set(SPDK_LD_FLAGS "-Wl,--whole-archive")
include_directories(${SPDK_DIR}/include)

# ---------------------------------------------------------
# Source files
# ---------------------------------------------------------

# Zeus lib os code
add_subdirectory(${ZEUS_SOURCE_DIR}/common)
add_subdirectory(${ZEUS_SOURCE_DIR}/librdma)
add_subdirectory(${ZEUS_SOURCE_DIR}/libposix)
add_subdirectory(${ZEUS_SOURCE_DIR}/libmtcp)
add_subdirectory(${ZEUS_SOURCE_DIR}/libdpdk)
add_subdirectory(${ZEUS_SOURCE_DIR}/liblwip)
add_subdirectory(${ZEUS_SOURCE_DIR}/libspdk)

# ---------------------------------------------------------
# Build targets
# ---------------------------------------------------------

# hoard target
add_custom_target(hoard ALL
                  COMMAND make
                  COMMAND mv ${HOARD_DIR}/libhoard.so ${ZEUS_DIR}
                  WORKING_DIRECTORY ${HOARD_DIR})

# POSIX libos target
add_library(zeus_posix SHARED
  ${SRC_ZEUS_COMMON_LIB} ${SRC_ZEUS_POSIX_LIB})
#target_link_libraries(zeus_posix ${ZEUS_DIR}/libhoard.so)
#add_dependencies(zeus_posix hoard)

# LWIP libos target
set( DPDK_LIB "-Wl,--whole-archive -lnuma -lpthread -ldl -lrte_mbuf  -lrte_eal -lrte_mempool -lrte_mempool_ring -lrte_ring -lrte_ethdev -lrte_kvargs -lrte_pmd_mlx5 -libverbs -Wl,--no-whole-archive" )
add_definitions(-mssse3 -D__STDC_LIMIT_MACROS)
add_library(zeus_lwip STATIC
  ${SRC_ZEUS_LWIP_LIB} ${SRC_ZEUS_COMMON_LIB})
target_link_libraries(zeus_lwip ${ZEUS_DIR}/libhoard.so ${DPDK_LIB})
add_dependencies(zeus_lwip hoard dpdk)

# MTCP libos target
add_library(zeus_mtcp STATIC
    ${MTCP_DIR}/mtcp/lib/libmtcp.a ${SRC_ZEUS_COMMON_LIB} ${SRC_ZEUS_MTCP_LIB})
target_link_libraries(zeus_mtcp ${ZEUS_DIR}/libhoard.so)
add_dependencies(zeus_mtcp hoard mtcp)

# RDMA libos target
add_library(zeus_rdma SHARED
  ${SRC_ZEUS_COMMON_LIB} ${SRC_ZEUS_RDMA_LIB})
target_link_libraries(zeus_rdma ${ZEUS_DIR}/libhoard.so rdmacm ibverbs)
add_dependencies(zeus_rdma hoard)

# spdk libos target
#set(CMAKE_C_FLAGS "${CMAKE_C_FLAGS} -Wl,--whole-archive")
#set(CMAKE_CXX_FLAGS "${CMAKE_CXX_FLAGS} -Wl,--whole-archive")
add_library(zeus_spdk STATIC
    ${SRC_ZEUS_COMMON_LIB} ${SRC_ZEUS_SPDK_LIB} ${SRC_ZEUS_POSIX_LIB})
target_link_libraries(zeus_spdk ${ZEUS_DIR}/libhoard.so)
add_dependencies(zeus_spdk hoard spdk)

#############################################
# applications
# Tapir
set(TAPIR_APP_DIR ${ZEUS_DIR}/apps/tapir)
add_custom_target(tapir
                  COMMAND ZEUS_SRC_DIR=${ZEUS_DIR} make
                  WORKING_DIRECTORY ${TAPIR_APP_DIR})
add_dependencies(tapir zeus_posix)

add_custom_target(tapir-clean
                  COMMAND make clean
                  WORKING_DIRECTORY ${TAPIR_APP_DIR})


# Redis
set(REDIS_APP_DIR ${ZEUS_DIR}/apps/redis)
add_custom_target(redis
                  COMMAND ZEUS_SRC_DIR=${ZEUS_DIR} make
                  WORKING_DIRECTORY ${REDIS_APP_DIR})


#add_subdirectory(apps/echo)

