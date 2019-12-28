cmake_minimum_required (VERSION 3.5.1)
project (HTTPSERV C CXX)

set(THREADS_PREFER_PTHREAD_FLAG ON)

find_package(Threads REQUIRED)
find_package(Boost REQUIRED COMPONENTS program_options chrono context system coroutine)
include_directories(${Boost_INCLUDE_DIR})

set(HTTP_APPS_DIR ${CMAKE_CURRENT_SOURCE_DIR})
set(COMMON_DIR ${HTTP_APPS_DIR}/../common/)

set(COMMON_SOURCES ${COMMON_DIR}/PspWorker.cc ${COMMON_DIR}/Request.cc)

set (HTTP_PARSER_SOURCES
    ${HTTP_APPS_DIR}/http_parser.c ${HTTP_APPS_DIR}/request_parser.c ${HTTP_APPS_DIR}/httpops.cc)
set(PSP_HTTP_SOURCES ${HTTP_APPS_DIR}/psp_http_server.cc ${COMMON_SOURCES} ${HTTP_PARSER_SOURCES})

configure_file(${CMAKE_CURRENT_SOURCE_DIR}/psp_srv_cfg.yaml ${CMAKE_CURRENT_BINARY_DIR}/psp_srv_cfg.yaml COPYONLY)

# LWIP HTTP SERVER
add_executable(psp-lwip-http-server ${PSP_HTTP_SOURCES})
target_link_libraries(psp-lwip-http-server psp-libos-common dmtr-libos-lwip pcre yaml-cpp)
