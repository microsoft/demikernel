# Zeus libOS code

cmake_minimum_required (VERSION 3.5.1)
project (EchoServer C CXX)

#set(RUNTIME_OUTPUT_DIRECTORY "execs")

set(THREADS_PREFER_PTHREAD_FLAG ON)
find_package(Threads REQUIRED)

set(ECHO_APPS_DIR ${CMAKE_CURRENT_SOURCE_DIR})

set(TCP_ECHO_SERVER_SOURCES ${ECHO_APPS_DIR}/tcp_server.cc)
set(TCP_ECHO_CLIENT_SOURCES ${ECHO_APPS_DIR}/tcp_client.cc)
set(UDP_ECHO_SERVER_SOURCES ${ECHO_APPS_DIR}/udp_server.cc)
set(UDP_ECHO_CLIENT_SOURCES ${ECHO_APPS_DIR}/udp_client.cc)
set(RAW_ECHO_SERVER_SOURCES
  ${ECHO_APPS_DIR}/dpdk_udp_echo_server.cpp
  ${ECHO_APPS_DIR}/dpdk_common.cpp
)
set(RAW_ECHO_CLIENT_SOURCES
  ${ECHO_APPS_DIR}/dpdk_udp_echo_client.cpp
  ${ECHO_APPS_DIR}/dpdk_common.cpp
)
set(RAW_MTCP_ECHO_SERVER_SOURCES
  ${ECHO_APPS_DIR}/mtcp_echo_server.cpp
  ${ECHO_APPS_DIR}/mtcp_common.cpp
)
set(RAW_MTCP_ECHO_CLIENT_SOURCES
  ${ECHO_APPS_DIR}/mtcp_echo_client.cpp
  ${ECHO_APPS_DIR}/mtcp_common.cpp
)
set(ZEUS_LATENCY_SOURCES
  ${CMAKE_SOURCE_DIR}/libos/common/latency.cc
  ${CMAKE_SOURCE_DIR}/libos/common/message.cc
  ${CMAKE_SOURCE_DIR}/libos/common/time_resolution.cc
)

# POSIX TCP server
add_executable(posix_server  ${TCP_ECHO_SERVER_SOURCES})
target_link_libraries(posix_server zeus_posix)

# POSIX TCP client
add_executable(posix_client  ${TCP_ECHO_CLIENT_SOURCES})
target_link_libraries(posix_client zeus_posix)

# POSIX TCP client & server
add_custom_target(posix)
add_dependencies(posix  posix_server posix_client)

# LWIP UDP server
add_executable(lwip_server ${UDP_ECHO_SERVER_SOURCES})
target_add_dpdk(lwip_server)
target_link_libraries(lwip_server zeus_lwip Threads::Threads)

# LWIP UDP client
add_executable(lwip_client  ${UDP_ECHO_CLIENT_SOURCES})
target_add_dpdk(lwip_client)
target_link_libraries(lwip_client zeus_lwip Threads::Threads)

# LWIP UDP client & server
add_custom_target(lwip)
add_dependencies(lwip lwip_server lwip_client)

# LWIP "raw" server
add_executable(lwip_raw_server ${RAW_ECHO_SERVER_SOURCES} ${ZEUS_LATENCY_SOURCES})
target_add_dpdk(lwip_raw_server)
target_link_libraries(lwip_raw_server zeus_lwip Threads::Threads)

# LWIP "raw" client
add_executable(lwip_raw_client ${RAW_ECHO_CLIENT_SOURCES} ${ZEUS_LATENCY_SOURCES})
target_add_dpdk(lwip_raw_client)
target_link_libraries(lwip_raw_client zeus_lwip Threads::Threads)

# LWIP "raw" client & server
add_custom_target(lwip_raw)
add_dependencies(lwip_raw lwip_raw_server lwip_raw_client)

if(MTCP_BUILD)
  # libMTCP server
  add_executable(mtcp_server ${TCP_ECHO_SERVER_SOURCES})
  target_add_mtcp(mtcp_server)
  target_link_libraries(mtcp_server zeus_mtcp Threads::Threads gmp)

  # libMTCP client
  add_executable(mtcp_client ${TCP_ECHO_CLIENT_SOURCES})
  target_add_mtcp(mtcp_client)
  target_link_libraries(mtcp_client zeus_mtcp Threads::Threads gmp)

  # libMTCP client & server
  add_custom_target(mtcp_echo)
  add_dependencies(mtcp_echo mtcp_server mtcp_client)

  # "raw" MTCP server
  add_executable(raw_mtcp_server ${RAW_MTCP_ECHO_SERVER_SOURCES} ${ZEUS_LATENCY_SOURCES})
  target_add_mtcp(raw_mtcp_server)
  target_link_libraries(raw_mtcp_server Threads::Threads gmp)

  # "raw" MTCP client
  add_executable(raw_mtcp_client ${RAW_MTCP_ECHO_CLIENT_SOURCES} ${ZEUS_LATENCY_SOURCES})
  target_add_mtcp(raw_mtcp_client)
  target_link_libraries(raw_mtcp_client Threads::Threads gmp)

  # "raw" MTCP client & server
  add_custom_target(raw_mtcp_echo)
  add_dependencies(raw_mtcp_echo raw_mtcp_server raw_mtcp_client)
endif(MTCP_BUILD)

# RDMA server
add_executable(rdma_server ${TCP_ECHO_SERVER_SOURCES})
target_link_libraries(rdma_server zeus_rdma rdmacm ibverbs)

# RDMA client
add_executable(rdma_client ${TCP_ECHO_CLIENT_SOURCES})
target_link_libraries(rdma_client zeus_rdma rdmacm ibverbs)

# RDMA client & server
add_custom_target(rdma)
add_dependencies(rdma rdma_server rdma_client)
