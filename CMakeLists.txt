# Copyright (c) Microsoft Corporation.
# Licensed under the MIT license.

cmake_minimum_required (VERSION 3.5.1)
project (Zeus C CXX)

set(CMAKE_MODULE_PATH ${CMAKE_MODULE_PATH} "${CMAKE_SOURCE_DIR}/scripts/cmake")

include(dpdk)
include(hoard)
include(redis)
include(tapir)
#include(spdk)

set(THREADS_PREFER_PTHREAD_FLAG ON)
find_package(Threads REQUIRED)

set(RELEASE_CFLAGS "-O3 -mtune=intel -DNDEBUG" CACHE STRING "C compiler options for release builds")
set(CMAKE_C_FLAGS_DEBUG "-Og")
set(CMAKE_C_FLAGS_RELEASE ${RELEASE_CFLAGS})
set(CMAKE_CXX_FLAGS_RELEASE ${RELEASE_CFLAGS})

# default CFLAGS
set(COMMON_CFLAGS "-g3 -fstack-protector -fPIC -Wall -Wextra -Wno-uninitialized -Wno-unused-parameter -Wl,-zdefs -fvisibility=hidden -I/usr/include/boost") # -lasan -fsanitize=address")
set(CMAKE_C_FLAGS "${CMAKE_C_FLAGS} ${COMMON_CFLAGS}")
set(CMAKE_CXX_FLAGS "${CMAKE_CXX_FLAGS} ${COMMON_CFLAGS} -std=c++17 -D__STDC_LIMIT_MACROS")

include_directories(
  ${CMAKE_CURRENT_SOURCE_DIR}/include
)

add_subdirectory(src)
