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

set(KV_SERVER_SOURCES ${KV_APPS_DIR}/kv_server.cc)

add_executable(dmtr-kv-server ${KV_SERVER_SOURCES})
target_link_libraries(dmtr-kv-server dmtr-libos-lwip pcre yaml-cpp boost_program_options Threads::Threads)
target_add_hoard(dmtr-kv-server hoard-vanilla)
