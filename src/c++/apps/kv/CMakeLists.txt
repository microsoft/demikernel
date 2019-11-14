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

set(KV_APPS_DIR ${CMAKE_CURRENT_SOURCE_DIR})
set(COMMON_SOURCES ${KV_APPS_DIR}/common.cc)

set(KV_SERVER_SOURCES ${KV_APPS_DIR}/kv_server.cc ${COMMON_SOURCES})
set(KV_CLIENT_SOURCES ${KV_APPS_DIR}/kv_client.cc ${COMMON_SOURCES})

add_executable(lwip-kv-server ${KV_SERVER_SOURCES})
target_link_libraries(lwip-kv-server dmtr-libos-lwip yaml-cpp boost_program_options Threads::Threads)
target_add_hoard(lwip-kv-server hoard-vanilla)

add_executable(posix-kv-server ${KV_SERVER_SOURCES})
target_link_libraries(posix-kv-server dmtr-libos-posix yaml-cpp boost_program_options Threads::Threads)
target_add_hoard(posix-kv-server hoard-vanilla)

add_executable(lwip-kv-client ${KV_CLIENT_SOURCES})
target_link_libraries(lwip-kv-client dmtr-libos-lwip yaml-cpp boost_program_options Threads::Threads)
target_add_hoard(lwip-kv-client hoard-vanilla)

add_executable(posix-kv-client ${KV_CLIENT_SOURCES})
target_link_libraries(posix-kv-client dmtr-libos-posix yaml-cpp boost_program_options Threads::Threads)
target_add_hoard(posix-kv-client hoard-vanilla)
