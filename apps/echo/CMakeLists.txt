# Zeus libOS code

cmake_minimum_required (VERSION 3.5.1)
project (EchoServer C CXX)

#set(RUNTIME_OUTPUT_DIRECTORY "execs")

set(THREADS_PREFER_PTHREAD_FLAG ON)

find_package(Threads REQUIRED)
find_package(Boost REQUIRED COMPONENTS
  program_options chrono)
include_directories(${Boost_INCLUDE_DIR})

set(ECHO_APPS_DIR ${CMAKE_CURRENT_SOURCE_DIR})

set(TCP_ECHO_SERVER_SOURCES ${ECHO_APPS_DIR}/tcp_server.cc)
set(TCP_ECHO_CLIENT_SOURCES ${ECHO_APPS_DIR}/tcp_client.cc)
set(UDP_ECHO_SERVER_SOURCES ${ECHO_APPS_DIR}/udp_server.cc)
set(UDP_ECHO_CLIENT_SOURCES ${ECHO_APPS_DIR}/udp_client.cc)
set(RAW_ECHO_SERVER_SOURCES ${ECHO_APPS_DIR}/posix_tcp_server.cc)
set(RAW_ECHO_CLIENT_SOURCES ${ECHO_APPS_DIR}/posix_tcp_client.cc)

# POSIX TCP server
add_executable(posix-echo-server ${TCP_ECHO_SERVER_SOURCES})
target_link_libraries(posix-echo-server dmtr-libos-posix yaml-cpp boost_program_options)

# POSIX TCP client
add_executable(posix-echo-client ${TCP_ECHO_CLIENT_SOURCES})
target_link_libraries(posix-echo-client dmtr-libos-posix yaml-cpp boost_program_options)

# POSIX TCP client & server
add_custom_target(posix-echo)
add_dependencies(posix-echo  posix-echo-server posix-echo-client)

# LWIP UDP server
add_executable(lwip-echo-server ${UDP_ECHO_SERVER_SOURCES})
target_link_libraries(lwip-echo-server dmtr-libos-lwip yaml-cpp boost_program_options)

# LWIP UDP client
add_executable(lwip-echo-client  ${UDP_ECHO_CLIENT_SOURCES})
target_link_libraries(lwip-echo-client dmtr-libos-lwip yaml-cpp boost_program_options)

# LWIP UDP client & server
add_custom_target(lwip-echo)
add_dependencies(lwip-echo lwip-echo-server lwip-echo-client)

# POSIX "raw" server
add_executable(posix_raw_server ${RAW_ECHO_SERVER_SOURCES})
target_link_libraries(posix_raw_server dmtr-libos-posix yaml-cpp boost_program_options)

# POSIX "raw" client
add_executable(posix_raw_client ${RAW_ECHO_CLIENT_SOURCES})
target_link_libraries(posix_raw_client dmtr-libos-posix yaml-cpp boost_program_options)

# POSIX "raw" client & server
add_custom_target(posix-raw)
add_dependencies(posix-raw posix_raw_server posix_raw_client)

if(BUILD_MTCP AND NOT AZURE_SUPPORT)
  # libMTCP server
  add_executable(mtcp_server ${TCP_ECHO_SERVER_SOURCES})
  target_link_libraries(mtcp_server dmtr-libos-mtcp Threads::Threads gmp)

  # libMTCP client
  add_executable(mtcp_client ${TCP_ECHO_CLIENT_SOURCES})
  target_link_libraries(mtcp_client dmtr-libos-mtcp Threads::Threads gmp)

  # libMTCP client & server
  add_custom_target(mtcp_echo)
  add_dependencies(mtcp_echo mtcp_server mtcp_client)

  # "raw" MTCP server
  add_executable(raw_mtcp_server ${RAW_MTCP_ECHO_SERVER_SOURCES})
  target_link_libraries(raw_mtcp_server Threads::Threads gmp)

  # "raw" MTCP client
  add_executable(raw_mtcp_client ${RAW_MTCP_ECHO_CLIENT_SOURCES})
  target_link_libraries(raw_mtcp_client Threads::Threads gmp)

  # "raw" MTCP client & server
  add_custom_target(raw_mtcp_echo)
  add_dependencies(raw_mtcp_echo raw_mtcp_server raw_mtcp_client)
endif(BUILD_MTCP AND NOT AZURE_SUPPORT)

# RDMA server
add_executable(rdma-echo-server ${TCP_ECHO_SERVER_SOURCES})
target_link_libraries(rdma-echo-server dmtr-libos-rdma rdmacm ibverbs yaml-cpp boost_program_options)

# RDMA client
add_executable(rdma-echo-client ${TCP_ECHO_CLIENT_SOURCES})
target_link_libraries(rdma-echo-client dmtr-libos-rdma rdmacm ibverbs yaml-cpp boost_program_options)

# RDMA client & server
add_custom_target(rdma-echo)
add_dependencies(rdma-echo rdma-echo-server rdma-echo-client)
