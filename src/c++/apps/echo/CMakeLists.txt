# Copyright (c) Microsoft Corporation.
# Licensed under the MIT license.

cmake_minimum_required (VERSION 3.5.1)
project (EchoServer C CXX)

#set(RUNTIME_OUTPUT_DIRECTORY "execs")

set(THREADS_PREFER_PTHREAD_FLAG ON)

find_package(Threads REQUIRED)
find_package(Boost REQUIRED COMPONENTS
  program_options chrono context system coroutine)
include_directories(${Boost_INCLUDE_DIR})

set(ECHO_APPS_DIR ${CMAKE_CURRENT_SOURCE_DIR})
set(COMMON_DIR ${ECHO_APPS_DIR}/../common/)

set(COMMON_SOURCES ${COMMON_DIR}/PspWorker.cc)

set(PSP_ECHO_SERVER_SOURCES ${ECHO_APPS_DIR}/psp_echo_server.cc)
set(PSP_ECHO_CLIENT_SOURCES ${ECHO_APPS_DIR}/psp_echo_client.cc)

configure_file(${CMAKE_CURRENT_SOURCE_DIR}/config.yaml ${CMAKE_CURRENT_BINARY_DIR}/config.yaml COPYONLY)
configure_file(${CMAKE_CURRENT_SOURCE_DIR}/psp_srv_cfg.yaml ${CMAKE_CURRENT_BINARY_DIR}/psp_srv_cfg.yaml COPYONLY)
configure_file(${CMAKE_CURRENT_SOURCE_DIR}/psp_clt_cfg.yaml ${CMAKE_CURRENT_BINARY_DIR}/psp_clt_cfg.yaml COPYONLY)

# LWIP PSP ECHO SERVER
add_executable(psp-lwip-echo-server ${PSP_ECHO_SERVER_SOURCES})
target_link_libraries(psp-lwip-echo-server psp-libos-common dmtr-libos-lwip yaml-cpp)

# LWIP PSP ECHO CLIENT
add_executable(psp-lwip-echo-client ${PSP_ECHO_CLIENT_SOURCES})
target_link_libraries(psp-lwip-echo-client psp-libos-common dmtr-libos-lwip yaml-cpp)
