# Zeus libOS code

cmake_minimum_required (VERSION 3.5.1)
project (Zeus C CXX)

set(CMAKE_MODULE_PATH ${CMAKE_MODULE_PATH} "${CMAKE_SOURCE_DIR}/scripts/cmake")

include(mtcp)
include(spdk)

set(THREADS_PREFER_PTHREAD_FLAG ON)
find_package(Threads REQUIRED)

# default CFLAGS
set(COMMON_CFLAGS "-g3 -mssse3 -fstack-protector -fPIC -Wall -Wextra -Wno-uninitialized -Wno-unused-parameter -Wl,-zdefs")
set(CMAKE_C_FLAGS "${CMAKE_C_FLAGS} ${COMMON_CFLAGS}")
set(CMAKE_CXX_FLAGS "${CMAKE_CXX_FLAGS}  ${COMMON_CFLAGS} -std=c++11 -D__STDC_LIMIT_MACROS")

# hoard
set(HOARD_SOURCE_DIR ${CMAKE_CURRENT_SOURCE_DIR}/libos/common/mem)
set(HOARD_LIBS ${HOARD_SOURCE_DIR}/libhoard.so)
add_custom_command(
  OUTPUT ${HOARD_LIBS}
  WORKING_DIRECTORY ${HOARD_SOURCE_DIR}
  COMMAND make
)
function(target_add_hoard TARGET)
  target_link_libraries(${TARGET} ${HOARD_LIBS})
  target_include_directories(${TARGET} PUBLIC
    ${HOARD_SOURCE_DIR}/include
    ${HOARD_SOURCE_DIR}/include/hoard
    ${HOARD_SOURCE_DIR}/include/superblocks
    ${HOARD_SOURCE_DIR}/include/util
    ${HOARD_SOURCE_DIR}/include/zeus
    ${HOARD_SOURCE_DIR}/Heap-Layers
  )
endfunction(target_add_hoard)

set(ZEUS_DIR ${CMAKE_CURRENT_SOURCE_DIR})

# todo: can these be compiled into a common library?
set(ZEUS_SOURCE_DIR ${ZEUS_DIR}/libos)
file(GLOB ZEUS_COMMON_SOURCES "${ZEUS_SOURCE_DIR}/common/*.cc")
include_directories(
  ${ZEUS_DIR}/include
  ${ZEUS_DIR}
)

# POSIX libos target
file(GLOB ZEUS_POSIX_SOURCES "${ZEUS_SOURCE_DIR}/libposix/*.cc")
add_library(zeus_posix SHARED ${ZEUS_COMMON_SOURCES} ${ZEUS_POSIX_SOURCES})

# LWIP libos target
file(GLOB ZEUS_LIBLWIP_SOURCES ${ZEUS_SOURCE_DIR}/liblwip/*.cc)
add_library(zeus_lwip STATIC ${ZEUS_LIBLWIP_SOURCES} ${ZEUS_COMMON_SOURCES})
target_add_hoard(zeus_lwip)
target_include_directories(zeus_lwip PUBLIC ${ZEUS_SOURCE_DIR}/liblwip ${DPDK_INCLUDE_DIR} )
# todo: these options should not be passed directly to
# `target_link_libraries()`. we should use `find_library()` instead.
target_link_libraries(zeus_lwip "-Wl,--whole-archive -lnuma -Wl,--no-whole-archive")
target_link_libraries(zeus_lwip Threads::Threads)
target_add_dpdk(zeus_lwip)

# MTCP libos target
file(GLOB ZEUS_MTCP_SOURCES "${ZEUS_SOURCE_DIR}/libmtcp/*.cc")
add_library(zeus_mtcp STATIC ${ZEUS_COMMON_SOURCES} ${ZEUS_MTCP_SOURCES})
target_add_hoard(zeus_mtcp)
target_add_mtcp(zeus_mtcp)

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

add_subdirectory(apps/echo)

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
