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


#add_subdirectory(apps/echo)

# ---------------------------------------------------------
# Include and link directories
# ---------------------------------------------------------

# todo: trigger submodule builds from within cmake.
set(SUBMODULE_DIR ${ZEUS_DIR}/submodules)
# todo: use a separate dpdk submodule for this variable.
set(DPDK_DIR ${SUBMODULE_DIR}/mtcp/dpdk-17.08 CACHE PATH "Path to DPDK")
set(MTCP_DIR ${SUBMODULE_DIR}/mtcp CACHE PATH "Path to MTCP")
set(SPDK_DIR ${SUBMODULE_DIR}/spdk CACHE PATH "Path to SPDK")

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
include_directories(${MTCP_DIR}/mtcp/include)

# LWIP
include_directories(${DPDK_DIR}/x86_64-native-linuxapp-gcc/include)
link_directories(${DPDK_DIR}/x86_64-native-linuxapp-gcc/lib)

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
add_dependencies(zeus_lwip hoard)

# MTCP libos target
add_library(zeus_mtcp STATIC
    ${MTCP_DIR}/mtcp/lib/libmtcp.a ${SRC_ZEUS_COMMON_LIB} ${SRC_ZEUS_MTCP_LIB})
target_link_libraries(zeus_mtcp ${ZEUS_DIR}/libhoard.so)
add_dependencies(zeus_mtcp hoard)

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
add_dependencies(zeus_spdk hoard)

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
