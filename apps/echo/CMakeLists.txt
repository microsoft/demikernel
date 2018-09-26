# Zeus libOS code

cmake_minimum_required (VERSION 3.5.1)
project (EchoServer C CXX)

set(ZEUS_DIR "../..")
set(DPDK_DIR ${ZEUS_DIR}/libos/libmtcp/mtcp/dpdk)
set(MTCP_DIR ${ZEUS_DIR}/libos/libmtcp/mtcp/mtcp)
set(RUNTIME_OUTPUT_DIRECTORY "execs")
# ----------------------------------------------------
# C Flags
# ----------------------------------------------------

set(CMAKE_C_FLAGS "${CMAKE_C_FLAGS} -g -fstack-protector -Wall \
  -Wno-uninitialized -O2 -DNDEBUG")
set(CMAKE_CXX_FLAGS "${CMAKE_CXX_FLAGS} -g -fstack-protector \
   -std=c++11 -fPIC -Wall -Wl,-zdefs")
set(CMAKE_LD_FLAGS "${CMAKE_LD_FLAGS}")

file(STRINGS ${DPDK_DIR}/include/cflags.txt DPDK_CFLAGS)
file(STRINGS ${DPDK_DIR}/lib/ldflags.txt DPDK_LDFLAGS)
set(ZEUS_LDFLAGS "-L${ZEUS_DIR}")
set(DPDK_LDFLAGS "${DPDK_LDFLAGS} -L${DPDK_DIR}/lib -libverbs")
set(MTCP_LDFLAGS "-L${MTCP_DIR}/lib")

# ---------------------------------------------------------
# Include and link directories
# ---------------------------------------------------------

include_directories(${PROJECT_SOURCE_DIR})
include_directories(${ZEUS_DIR}/include)
include_directories(${ZEUS_DIR}/libos)
link_directories(${ZEUS_DIR})
set(THREADS_PREFER_PTHREAD_FLAG ON)
find_package(Threads REQUIRED)

# ---------------------------------------------------------
# Source files
# ---------------------------------------------------------

set(TCP_SERVER "tcp_server.cc")
set(TCP_CLIENT "tcp_client.cc")
set(UDP_ECHO_SERVER "udp_server.cc")
set(UDP_ECHO_CLIENT "udp_client.cc")
set(ZEUS_LATENCY
  "${ZEUS_DIR}/libos/common/latency.cc"
  "${ZEUS_DIR}/libos/common/message.cc"
  "${ZEUS_DIR}/libos/common/time_resolution.cc")

# ---------------------------------------------------------
# Build targets
# ---------------------------------------------------------

# POSIX TCP server
add_executable(posix_server
  ${TCP_SERVER})
add_executable(posix_client
  ${TCP_CLIENT})
# set_target_properties(posix_server PROPERTIES
#   LINK_FLAGS "${ZEUS_LDFLAGS}")
# set_target_properties(posix_client PROPERTIES
#   LINK_FLAGS "${ZEUS_LDFLAGS}")

target_link_libraries(posix_server
  zeus_posix
  hoard)
target_link_libraries(posix_client 
  zeus_posix
  hoard)
add_custom_target(posix)
add_dependencies(posix
  posix_server
  posix_client)

# LWIP UDP server
add_executable(lwip_server
  ${UDP_ECHO_SERVER})
add_executable(lwip_client
  ${UDP_ECHO_CLIENT})
set_target_properties(lwip_client PROPERTIES
  COMPILE_FLAGS "${DPDK_CFLAGS} ${DPDK_LDFLAGS}"
  LINK_FLAGS "${DPDK_LDFLAGS} ${ZEUS_LDFLAGS}")

set_target_properties(lwip_server PROPERTIES
  COMPILE_FLAGS "${DPDK_CFLAGS} ${DPDK_LDFLAGS}"
  LINK_FLAGS "${DPDK_LDFLAGS} ${ZEUS_LDFLAGS}")

target_link_libraries(lwip_server
  zeus_lwip
  hoard
  Threads::Threads)
target_link_libraries(lwip_client 
  zeus_lwip
  hoard
  Threads::Threads)

target_include_directories(lwip_server PRIVATE
  ${DPDK_DIR}/include
  ${DPDK_DIR}/lib)
target_include_directories(lwip_client PRIVATE
  ${DPDK_DIR}/include
  ${DPDK_DIR}/lib)

add_custom_target(lwip)
add_dependencies(lwip
  lwip_server
  lwip_client)

# LWIP "raw" server
add_executable(lwip_raw_server
  "dpdk_udp_echo_server.cpp" "dpdk_common.cpp" ${ZEUS_LATENCY})
add_executable(lwip_raw_client
  "dpdk_udp_echo_client.cpp" "dpdk_common.cpp" ${ZEUS_LATENCY})
set_target_properties(lwip_raw_client PROPERTIES
  COMPILE_FLAGS "${DPDK_CFLAGS} ${DPDK_LDFLAGS}"
  LINK_FLAGS "${DPDK_LDFLAGS} ${ZEUS_LDFLAGS}")

set_target_properties(lwip_raw_server PROPERTIES
  COMPILE_FLAGS "${DPDK_CFLAGS} ${DPDK_LDFLAGS}"
  LINK_FLAGS "${DPDK_LDFLAGS} ${ZEUS_LDFLAGS}")

target_link_libraries(lwip_raw_server
  hoard
  Threads::Threads)
target_link_libraries(lwip_raw_client
  hoard
  Threads::Threads)

target_include_directories(lwip_raw_server PRIVATE
  ${DPDK_DIR}/include
  ${DPDK_DIR}/lib)
target_include_directories(lwip_raw_client PRIVATE
  ${DPDK_DIR}/include
  ${DPDK_DIR}/lib)

add_custom_target(lwip_raw)
add_dependencies(lwip_raw
  lwip_raw_server
  lwip_raw_client)

# libMTCP server
add_executable(mtcp_server
  ${TCP_SERVER})
add_executable(mtcp_client
  ${TCP_CLIENT})
set_target_properties(mtcp_client PROPERTIES
  COMPILE_FLAGS "${DPDK_CFLAGS} ${MTCP_LDFLAGS}"
  LINK_FLAGS "${DPDK_LDFLAGS} ${MTCP_LDFLAGS} ${ZEUS_LDFLAGS}")

set_target_properties(mtcp_server PROPERTIES
  COMPILE_FLAGS "${DPDK_CFLAGS} ${MTCP_LDFLAGS}"
  LINK_FLAGS "${DPDK_LDFLAGS} ${MTCP_LDFLAGS} ${ZEUS_LDFLAGS}")

target_link_libraries(mtcp_server
  zeus_mtcp
  hoard
  Threads::Threads
  mtcp
  gmp)
target_link_libraries(mtcp_client 
  zeus_mtcp
  hoard
  Threads::Threads
  mtcp
  gmp)

target_include_directories(mtcp_server PRIVATE
  ${DPDK_DIR}/include
  ${DPDK_DIR}/lib
  ${MTCP_DIR}/lib)
target_include_directories(mtcp_client PRIVATE
  ${DPDK_DIR}/include
  ${DPDK_DIR}/lib
  ${MTCP_DIR}/lib)

add_custom_target(mtcp_echo)
add_dependencies(mtcp_echo
  mtcp_server
  mtcp_client)

# "raw" MTCP server
add_executable(raw_mtcp_server
  "mtcp_echo_server.cpp" "mtcp_common.cpp" ${ZEUS_LATENCY})
add_executable(raw_mtcp_client
  "mtcp_echo_client.cpp" "mtcp_common.cpp" ${ZEUS_LATENCY})
set_target_properties(raw_mtcp_client PROPERTIES
  COMPILE_FLAGS "${DPDK_CFLAGS} ${MTCP_LDFLAGS}"
  LINK_FLAGS "${DPDK_LDFLAGS} ${MTCP_LDFLAGS} ${ZEUS_LDFLAGS}")

set_target_properties(raw_mtcp_server PROPERTIES
  COMPILE_FLAGS "${DPDK_CFLAGS} ${MTCP_LDFLAGS}"
  LINK_FLAGS "${DPDK_LDFLAGS} ${MTCP_LDFLAGS} ${ZEUS_LDFLAGS}")

target_link_libraries(raw_mtcp_server
  hoard
  Threads::Threads
  mtcp
  gmp)
target_link_libraries(raw_mtcp_client
  hoard
  Threads::Threads
  mtcp
  gmp)

target_include_directories(raw_mtcp_server PRIVATE
  ${DPDK_DIR}/include
  ${DPDK_DIR}/lib
  ${MTCP_DIR}/include
  ${MTCP_DIR}/lib)
target_include_directories(raw_mtcp_client PRIVATE
  ${DPDK_DIR}/include
  ${DPDK_DIR}/lib
  ${MTCP_DIR}/include
  ${MTCP_DIR}/lib)

add_custom_target(raw_mtcp_echo)
add_dependencies(raw_mtcp_echo
  raw_mtcp_server
  raw_mtcp_client)

# # "original" MTCP server
# add_executable(orig_mtcp_server
#   "mtcp_echo_server.cpp" "mtcp_common.cpp" )
# add_executable(orig_mtcp_client
#   "mtcp_echo_client.cpp" "orig_mtcp_common.cpp")
# set_target_properties(orig_mtcp_client PROPERTIES
#   COMPILE_FLAGS "${DPDK_CFLAGS} ${ORIG_MTCP_LDFLAGS}"
#   LINK_FLAGS "${DPDK_LDFLAGS} ${ORIG_MTCP_LDFLAGS} ${ZEUS_LDFLAGS}")

# set_target_properties(orig_mtcp_server PROPERTIES
#   COMPILE_FLAGS "${DPDK_CFLAGS} ${ORIG_MTCP_LDFLAGS}"
#   LINK_FLAGS "${DPDK_LDFLAGS} ${ORIG_MTCP_LDFLAGS} ${ZEUS_LDFLAGS}")

# target_link_libraries(orig_mtcp_server
#   zeus_mtcp
#   hoard
#   Threads::Threads
#   mtcp
#   gmp)
# target_link_libraries(orig_mtcp_client 
#   zeus_mtcp
#   hoard
#   Threads::Threads
#   mtcp
#   gmp)

# target_include_directories(orig_mtcp_server PRIVATE
#   ${DPDK_DIR}/include
#   ${DPDK_DIR}/lib
#   ${ORIG_MTCP_DIR}/lib)
# target_include_directories(orig_mtcp_client PRIVATE
#   ${DPDK_DIR}/include
#   ${DPDK_DIR}/lib
#   ${ORIG_MTCP_DIR}/lib)

# add_custom_target(orig_mtcp_echo)
# add_dependencies(orig_mtcp_echo
#   orig_mtcp_server
#   orig_mtcp_client)

# RDMA server
add_executable(rdma_server
  ${TCP_SERVER})
add_executable(rdma_client
  ${TCP_CLIENT})
# set_target_properties(rdma_server PROPERTIES
#   LINK_FLAGS "${ZEUS_LDFLAGS}")
# set_target_properties(rdma_client PROPERTIES
#   LINK_FLAGS "${ZEUS_LDFLAGS}")

target_link_libraries(rdma_server
  zeus_rdma
  hoard
  rdmacm
  ibverbs)
target_link_libraries(rdma_client 
  zeus_rdma
  hoard
  rdmacm
  ibverbs)
add_custom_target(rdma)
add_dependencies(rdma
  rdma_server
  rdma_client)
