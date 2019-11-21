# Copyright (c) Microsoft Corporation.
# Licensed under the MIT license.

cmake_minimum_required (VERSION 3.5.1)
project (KVStore C CXX)

set(RUNTIME_OUTPUT_DIRECTORY "execs")

set(THREADS_PREFER_PTHREAD_FLAG ON)

find_package(Threads REQUIRED)
find_package(Boost REQUIRED COMPONENTS
  program_options chrono context system coroutine)
include_directories(${Boost_INCLUDE_DIR})

set(NC_APPS_DIR ${CMAKE_CURRENT_SOURCE_DIR})

set(NC_CLIENT_SOURCES ${NC_APPS_DIR}/nc.cc)

add_executable(lwip-nc-client ${NC_CLIENT_SOURCES})
target_link_libraries(lwip-nc-client dmtr-libos-lwip yaml-cpp boost_program_options Threads::Threads)
target_add_hoard(lwip-nc-client hoard-vanilla)

add_executable(posix-nc-client ${NC_CLIENT_SOURCES})
target_link_libraries(posix-nc-client dmtr-libos-posix yaml-cpp boost_program_options Threads::Threads)
target_add_hoard(posix-nc-client hoard-vanilla)
