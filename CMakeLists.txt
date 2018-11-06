# Zeus libOS code

cmake_minimum_required (VERSION 3.5.1)
project (Zeus C CXX)

set(CMAKE_MODULE_PATH ${CMAKE_MODULE_PATH} "${CMAKE_SOURCE_DIR}/scripts/cmake")

include(mtcp)
include(spdk)

set(THREADS_PREFER_PTHREAD_FLAG ON)
find_package(Threads REQUIRED)

# default CFLAGS
set(COMMON_CFLAGS "-g3 -mssse3 -fstack-protector -fPIC -Wall -Wextra -Wno-uninitialized -Wno-unused-parameter -Wl,-zdefs")
set(CMAKE_C_FLAGS "${CMAKE_C_FLAGS} ${COMMON_CFLAGS}")
set(CMAKE_CXX_FLAGS "${CMAKE_CXX_FLAGS}  ${COMMON_CFLAGS} -std=c++11 -D__STDC_LIMIT_MACROS")

# hoard
set(HOARD_SOURCE_DIR ${CMAKE_CURRENT_SOURCE_DIR}/libos/common/mem)
set(HOARD_LIBS ${HOARD_SOURCE_DIR}/libhoard.so)
add_custom_command(
  OUTPUT ${HOARD_LIBS}
  WORKING_DIRECTORY ${HOARD_SOURCE_DIR}
  COMMAND make
)
function(target_add_hoard TARGET)
  target_link_libraries(${TARGET} ${HOARD_LIBS})
  target_include_directories(${TARGET} PUBLIC
    ${HOARD_SOURCE_DIR}/include
    ${HOARD_SOURCE_DIR}/include/hoard
    ${HOARD_SOURCE_DIR}/include/superblocks
    ${HOARD_SOURCE_DIR}/include/util
    ${HOARD_SOURCE_DIR}/include/zeus
    ${HOARD_SOURCE_DIR}/Heap-Layers
  )
endfunction(target_add_hoard)

include_directories(
  ${CMAKE_CURRENT_SOURCE_DIR}/include
  ${CMAKE_CURRENT_SOURCE_DIR}
)

add_subdirectory(libos)
add_subdirectory(apps)
