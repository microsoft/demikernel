# Zeus libOS code

cmake_minimum_required (VERSION 3.5.1)
project (Zeus C CXX)

set(CMAKE_MODULE_PATH ${CMAKE_MODULE_PATH} "${CMAKE_SOURCE_DIR}/scripts/cmake")

include(dpdk)
include(hoard)
include(mtcp)
#include(spdk)

set(THREADS_PREFER_PTHREAD_FLAG ON)
find_package(Threads REQUIRED)

# default CFLAGS
set(COMMON_CFLAGS "-g3 -mssse3 -fstack-protector -fPIC -Wall -Wextra -Wno-uninitialized -Wno-unused-parameter -Wl,-zdefs")
set(CMAKE_C_FLAGS "${CMAKE_C_FLAGS} ${COMMON_CFLAGS}")
set(CMAKE_CXX_FLAGS "${CMAKE_CXX_FLAGS}  ${COMMON_CFLAGS} -std=c++11 -D__STDC_LIMIT_MACROS")

include_directories(
  ${CMAKE_CURRENT_SOURCE_DIR}/include
  ${CMAKE_CURRENT_SOURCE_DIR}
)

add_subdirectory(libos)
add_subdirectory(apps)
