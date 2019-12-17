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

set (HTTP_PARSER_SOURCES
    ${ECHO_APPS_DIR}/http_parser.c ${ECHO_APPS_DIR}/request_parser.c ${ECHO_APPS_DIR}/httpops.cc)
set(HTTP_THREADED_SERVER_SOURCES
    ${ECHO_APPS_DIR}/dmtr_http_server_threaded.cc ${HTTP_PARSER_SOURCES})
set(HTTP_RATE_CLIENT_SOURCES ${ECHO_APPS_DIR}/dmtr_rate_client.cc)
set(HTTP_CL_CLIENT_SOURCES ${ECHO_APPS_DIR}/dmtr_cl_client.cc)
set(PSP_ECHO_SERVER_SOURCES ${ECHO_APPS_DIR}/psp_echo_server.cc)
set(PSP_ECHO_CLIENT_SOURCES ${ECHO_APPS_DIR}/psp_echo_client.cc)
set(PSP_ECHO_CLCLIENT_SOURCES ${ECHO_APPS_DIR}/psp_echo_clclient.cc)

configure_file(${CMAKE_CURRENT_SOURCE_DIR}/config.yaml ${CMAKE_CURRENT_BINARY_DIR}/config.yaml COPYONLY)
configure_file(${CMAKE_CURRENT_SOURCE_DIR}/psp_srv_cfg.yaml ${CMAKE_CURRENT_BINARY_DIR}/psp_srv_cfg.yaml COPYONLY)
configure_file(${CMAKE_CURRENT_SOURCE_DIR}/psp_clt_cfg.yaml ${CMAKE_CURRENT_BINARY_DIR}/psp_clt_cfg.yaml COPYONLY)

# POSIX HTTP server threaded
add_executable(dmtr-posix-http-server-threaded ${HTTP_THREADED_SERVER_SOURCES})
target_link_libraries(dmtr-posix-http-server-threaded dmtr-libos-posix pcre yaml-cpp Threads::Threads)

# POSIX HTTP rate client
add_executable(dmtr-posix-http-rate-client ${HTTP_RATE_CLIENT_SOURCES})
target_link_libraries(dmtr-posix-http-rate-client dmtr-libos-posix yaml-cpp boost_program_options Threads::Threads)

# LWIP HTTP server threaded
add_executable(dmtr-lwip-http-server-threaded ${HTTP_THREADED_SERVER_SOURCES})
target_link_libraries(dmtr-lwip-http-server-threaded dmtr-libos-lwip pcre yaml-cpp Threads::Threads)
target_add_hoard(dmtr-lwip-http-server-threaded hoard-vanilla)

# LWIP HTTP rate client
add_executable(dmtr-lwip-http-rate-client ${HTTP_RATE_CLIENT_SOURCES})
target_link_libraries(dmtr-lwip-http-rate-client dmtr-libos-lwip yaml-cpp Threads::Threads)
target_add_hoard(dmtr-lwip-http-rate-client hoard-vanilla)

# LWIP HTTP closed loop client
add_executable(dmtr-lwip-http-cl-client ${HTTP_CL_CLIENT_SOURCES})
target_link_libraries(dmtr-lwip-http-cl-client psp-libos-common dmtr-libos-lwip yaml-cpp boost_program_options)
target_add_hoard(dmtr-lwip-http-rate-client hoard-vanilla)

# LWIP PSP ECHO SERVER
add_executable(psp-lwip-echo-server ${PSP_ECHO_SERVER_SOURCES})
target_link_libraries(psp-lwip-echo-server psp-libos-common dmtr-libos-lwip yaml-cpp)

# LWIP PSP ECHO CLIENT
add_executable(psp-lwip-echo-client ${PSP_ECHO_CLIENT_SOURCES})
target_link_libraries(psp-lwip-echo-client psp-libos-common dmtr-libos-lwip yaml-cpp)
