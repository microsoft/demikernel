cmake_minimum_required (VERSION 3.5.1)
project (Clients C CXX)

set(THREADS_PREFER_PTHREAD_FLAG ON)

find_package(Threads REQUIRED)
find_package(Boost REQUIRED COMPONENTS program_options chrono context system coroutine)
include_directories(${Boost_INCLUDE_DIR})

set(CLIENT_APPS_DIR ${CMAKE_CURRENT_SOURCE_DIR})
set(COMMON_DIR ${CLIENT_APPS_DIR}/../common/)

set(COMMON_SOURCES ${COMMON_DIR}/PspWorker.cc ${COMMON_DIR}/Request.cc)

set(PSP_CL_CLIENT_SOURCES ${CLIENT_APPS_DIR}/psp_cl_client.cc ${COMMON_SOURCES})

configure_file(${CMAKE_CURRENT_SOURCE_DIR}/psp_clt_cfg.yaml ${CMAKE_CURRENT_BINARY_DIR}/psp_clt_cfg.yaml COPYONLY)

# LWIP CLOSED LOOP CLIENT
add_executable(psp-lwip-cl-client ${PSP_CL_CLIENT_SOURCES})
target_link_libraries(psp-lwip-cl-client psp-libos-common dmtr-libos-lwip yaml-cpp)
