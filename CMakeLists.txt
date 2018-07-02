# Zeus libOS code

cmake_minimum_required (VERSION 3.7.2)
project (Zeus C CXX)

# ----------------------------------------------------
# Additional CMake modules 
# ----------------------------------------------------

include(ExternalProject)
set(CMAKE_MODULE_PATH ${CMAKE_MODULE_PATH}
  "${CMAKE_SOURCE_DIR}/cmake/Modules/")

# ----------------------------------------------------
# C Flags
# ----------------------------------------------------

set(CMAKE_C_FLAGS "${CMAKE_C_FLAGS} -g -fstack-protector -Wall \
  -Wno-uninitialized -O3 -DNASSERT")
set(CMAKE_CXX_FLAGS "${CMAKE_CXX_FLAGS} -g -fstack-protector \
   -std=c++11 -fPIC -Wall -Wl,-zdefs")
   set(CMAKE_LD_FLAGS "${CMAKE_LD_FLAGS}")

# ---------------------------------------------------------
# Include and link directories
# ---------------------------------------------------------

set(ZEUS_SOURCE_DIR ${PROJECT_SOURCE_DIR}/libos)
include_directories(${PROJECT_SOURCE_DIR})
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
set(DPDK_DIR ${ZEUS_SOURCE_DIR}/libmtcp/mtcp/dpdk/)
set(MTCP_DIR ${ZEUS_SOURCE_DIR}/libmtcp/mtcp/mtcp/)
include_directories(${MTCP_DIR}/include)
#link_directories(${MTCP_DIR}/lib)
#include_directories(${DPDK_DIR}/include)
#link_directories(${DPDK_DIR}/lib)
#execute_process(COMMAND cat ${DPDK_DIR}/lib/ldflags.txt OUTPUT_VARIABLE DPDK_LD_FLAGS)
#execute_process(COMMAND cat ${DPDK_DIR}/include/cflags.txt OUTPUT_VARIABLE DPDK_CFLAGS)

# ---------------------------------------------------------
# Source files
# ---------------------------------------------------------

# Zeus lib os code
add_subdirectory(${ZEUS_SOURCE_DIR}/common)
add_subdirectory(${ZEUS_SOURCE_DIR}/librdma)
add_subdirectory(${ZEUS_SOURCE_DIR}/libposix)
add_subdirectory(${ZEUS_SOURCE_DIR}/libmtcp)
add_subdirectory(${ZEUS_SOURCE_DIR}/libdpdk)

# ---------------------------------------------------------
# Build targets
# ---------------------------------------------------------

# hoard target
add_custom_target(hoard ALL
                  COMMAND make
                  COMMAND mv libhoard.* ${PROJECT_SOURCE_DIR}
                  WORKING_DIRECTORY ${HOARD_DIR})

# POSIX libos target
add_library(zeus_posix SHARED
  ${SRC_ZEUS_COMMON_LIB} ${SRC_ZEUS_POSIX_LIB})
target_link_libraries(zeus_posix ${PROJECT_SOURCE_DIR}/libhoard.so)
add_dependencies(zeus_posix hoard)

# MTCP libos target
add_library(zeus_mtcp STATIC
    ${MTCP_DIR}/lib/libmtcp.a ${SRC_ZEUS_COMMON_LIB} ${SRC_ZEUS_MTCP_LIB})
#set_target_properties(zeus_mtcp PROPERTIES COMPILE_FLAGS ${DPDK_CFLAGS})
target_link_libraries(zeus_mtcp ${PROJECT_SOURCE_DIR}/libhoard.so)
#set_target_properties(zeus_mtcp PROPERTIES LINK_FLAGS "-pthread -lrt -march=native -export-dynamic")
#set_target_properties(zeus_mtcp PROPERTIES LINK_FLAGS ${MTCP_DIR}/lib/libmtcp.a)
#target_link_libraries(zeus_mtcp ${MTCP_DIR}/lib/libmtcp.a)
#set_target_properties(zeus_mtcp PROPERTIES LINK_FLAGS "-lnuma -lmtcp -lpthread -lrt -ld")
#set_target_properties(zeus_mtcp PROPERTIES LINK_FLAGS ${DPDK_LD_FLAGS})
add_dependencies(zeus_mtcp hoard)

# RDMA libos target
if (NOT ${APPLE})
set(CMAKE_C_FLAGS "${CMAKE_C_FLAGS} -lrdmacm -libverbs")
set(CMAKE_C_FLAGS "${CMAKE_CXX_FLAGS} -lrdmacm -libverbs")
add_library(zeus_rdma SHARED ${SRC_ZEUS_RDMA_LIB})
target_link_libraries(zeus_rdma ${PROJECT_SOURCE_DIR}/libhoard.so)
endif()


set(TAPIR_APP_DIR ${PROJECT_SOURCE_DIR}/apps/tapir)
add_custom_target(tapir
                  COMMAND ZEUS_SRC_DIR=${PROJECT_SOURCE_DIR} make
                  WORKING_DIRECTORY ${TAPIR_APP_DIR})
add_dependencies(tapir zeus_posix)
                
add_custom_target(tapir-clean
                  COMMAND make clean
                  WORKING_DIRECTORY ${TAPIR_APP_DIR})

set(TAPIR_APP_DIR ${PROJECT_SOURCE_DIR}/apps/redis)
add_custom_target(redis
                  COMMAND ZEUS_SRC_DIR=${PROJECT_SOURCE_DIR} make
                  WORKING_DIRECTORY ${TAPIR_APP_DIR})


