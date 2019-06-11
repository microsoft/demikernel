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

set (HTTP_PARSER_SOURCES
    ${ECHO_APPS_DIR}/http_parser.c ${ECHO_APPS_DIR}/request_parser.c ${ECHO_APPS_DIR}/httpops.cc)
set(HTTP_THREADED_SERVER_SOURCES
    ${ECHO_APPS_DIR}/dmtr_http_server_threaded.cc ${HTTP_PARSER_SOURCES})
set(HTTP_SERVER_SOURCES
    ${ECHO_APPS_DIR}/dmtr_http_server.cc ${HTTP_PARSER_SOURCES})
set(HTTP_RATE_CLIENT_SOURCES ${ECHO_APPS_DIR}/rate_client.cc)
set(TCP_ECHO_SERVER_SOURCES ${ECHO_APPS_DIR}/dmtr_tcp_server.cc)
set(TCP_ECHO_CLIENT_SOURCES ${ECHO_APPS_DIR}/dmtr_tcp_client.cc)
set(UDP_ECHO_SERVER_SOURCES ${ECHO_APPS_DIR}/dmtr_udp_server.cc)
set(UDP_ECHO_CLIENT_SOURCES ${ECHO_APPS_DIR}/dmtr_udp_client.cc)
set(RAW_ECHO_SERVER_SOURCES ${ECHO_APPS_DIR}/posix_tcp_server.cc)
set(RAW_ECHO_CLIENT_SOURCES ${ECHO_APPS_DIR}/posix_tcp_client.cc)

configure_file(${CMAKE_CURRENT_SOURCE_DIR}/config.yaml ${CMAKE_CURRENT_BINARY_DIR}/config.yaml COPYONLY)

# POSIX HTTP server threaded
add_executable(dmtr-posix-http-server-threaded ${HTTP_THREADED_SERVER_SOURCES})
target_link_libraries(dmtr-posix-http-server-threaded dmtr-libos-posix pcre yaml-cpp boost_program_options Threads::Threads)

# POSIX HTTP rate client
add_executable(dmtr-posix-http-rate-client ${HTTP_RATE_CLIENT_SOURCES})
target_link_libraries(dmtr-posix-http-rate-client dmtr-libos-posix yaml-cpp boost_program_options Threads::Threads)

# POSIX HTTP server
add_executable(dmtr-posix-http-server ${HTTP_SERVER_SOURCES})
target_link_libraries(dmtr-posix-http-server dmtr-libos-posix pcre yaml-cpp boost_program_options)

# POSIX TCP server
add_executable(dmtr-posix-server ${TCP_ECHO_SERVER_SOURCES})
target_link_libraries(dmtr-posix-server dmtr-libos-posix yaml-cpp boost_program_options)

# POSIX TCP client
add_executable(dmtr-posix-client ${TCP_ECHO_CLIENT_SOURCES})
target_link_libraries(dmtr-posix-client dmtr-libos-posix yaml-cpp boost_program_options)

# POSIX TCP client & server
add_custom_target(dmtr-posix-echo)
add_dependencies(dmtr-posix-echo  dmtr-posix-server dmtr-posix-client)

# LWIP TCP server
add_executable(dmtr-lwip-server ${TCP_ECHO_SERVER_SOURCES})
target_link_libraries(dmtr-lwip-server dmtr-libos-lwip yaml-cpp boost_program_options)

# LWIP TCP client
add_executable(dmtr-lwip-client  ${TCP_ECHO_CLIENT_SOURCES})
target_link_libraries(dmtr-lwip-client dmtr-libos-lwip yaml-cpp boost_program_options)

# LWIP TCP client & server
add_custom_target(dmtr-lwip-echo)
add_dependencies(dmtr-lwip-echo dmtr-lwip-server dmtr-lwip-client)

# POSIX "raw" non-demeter server
add_executable(posix-server ${RAW_ECHO_SERVER_SOURCES})
target_link_libraries(posix-server dmtr-latency dmtr-libos-common yaml-cpp boost_program_options)

# POSIX "raw" non-demeter client
add_executable(posix-client ${RAW_ECHO_CLIENT_SOURCES})
target_link_libraries(posix-client dmtr-latency dmtr-libos-common yaml-cpp boost_program_options)

# POSIX "raw" client & server
add_custom_target(posix-echo)
add_dependencies(posix-echo posix-server posix-client)

# RDMA server
add_executable(dmtr-rdma-server ${TCP_ECHO_SERVER_SOURCES})
target_link_libraries(dmtr-rdma-server dmtr-libos-rdma rdmacm ibverbs yaml-cpp boost_program_options)

# RDMA client
add_executable(dmtr-rdma-client ${TCP_ECHO_CLIENT_SOURCES})
target_link_libraries(dmtr-rdma-client dmtr-libos-rdma rdmacm ibverbs yaml-cpp boost_program_options)

# RDMA client & server
add_custom_target(dmtr-rdma-echo)
add_dependencies(dmtr-rdma-echo dmtr-rdma-server dmtr-rdma-client)

add_custom_target(echo)
add_dependencies(echo posix-echo dmtr-posix-echo dmtr-lwip-echo dmtr-rdma-echo)
