# Zeus libOS code

cmake_minimum_required (VERSION 3.7.2)
project (Zeus C CXX)

# ----------------------------------------------------
# Additional CMake modules 
# ----------------------------------------------------

include(ExternalProject)
set(CMAKE_MODULE_PATH ${CMAKE_MODULE_PATH}
  "${CMAKE_SOURCE_DIR}/cmake/Modules/")

# ----------------------------------------------------
# C Flags
# ----------------------------------------------------

set(CMAKE_C_FLAGS "${CMAKE_C_FLAGS} -g -fstack-protector -Wall \
  -Wno-uninitialized -O3 -DNASSERT")
set(CMAKE_CXX_FLAGS "${CMAKE_CXX_FLAGS} -g -fstack-protector \
   -std=c++11 -fPIC -Wall")
   set(CMAKE_LD_FLAGS "${CMAKE_LD_FLAGS}")

# ---------------------------------------------------------
# Include and link directories
# ---------------------------------------------------------

set(ZEUS_SOURCE_DIR ${PROJECT_SOURCE_DIR}/libos)
include_directories(${PROJECT_SOURCE_DIR})
include_directories(${ZEUS_SOURCE_DIR})



# Hoard
Set(HOARD_DIR ${ZEUS_SOURCE_DIR}/common/mem)
include_directories(${HOARD_DIR}/include)
include_directories(${HOARD_DIR}/include/hoard)
include_directories(${HOARD_DIR}/include/superblocks)
include_directories(${HOARD_DIR}/include/util)
include_directories(${HOARD_DIR}/include/zeus)
include_directories(${HOARD_DIR}/Heap-Layers)

# ---------------------------------------------------------
# Source files
# ---------------------------------------------------------

# Zeus lib os code
add_subdirectory(${ZEUS_SOURCE_DIR}/common)
add_subdirectory(${ZEUS_SOURCE_DIR}/librdma)
add_subdirectory(${ZEUS_SOURCE_DIR}/libposix)
add_subdirectory(${ZEUS_SOURCE_DIR}/libdpdk)

# ---------------------------------------------------------
# Build targets
# ---------------------------------------------------------

# hoard target
add_custom_target(hoard ALL
                  COMMAND make
                  COMMAND mv libhoard.* ${PROJECT_SOURCE_DIR}
                  WORKING_DIRECTORY ${HOARD_DIR})

# POSIX libos target
add_library(zeus_posix SHARED
  ${SRC_ZEUS_POSIX_LIB} ${SRC_ZEUS_COMMON_LIB})
target_link_libraries(zeus_posix ${PROJECT_SOURCE_DIR}/libhoard.so)
add_dependencies(zeus_posix hoard)

# RDMA libos target
if (NOT ${APPLE})
set(CMAKE_C_FLAGS "${CMAKE_C_FLAGS} -lrdmacm -libverbs")
set(CMAKE_C_FLAGS "${CMAKE_CXX_FLAGS} -lrdmacm -libverbs")
add_library(zeus_rdma SHARED ${SRC_ZEUS_RDMA_LIB})
target_link_libraries(zeus_rdma ${PROJECT_SOURCE_DIR}/libhoard.so)
endif()


set(TAPIR_APP_DIR ${PROJECT_SOURCE_DIR}/apps/tapir)
add_custom_target(tapir
                  COMMAND ZEUS_SRC_DIR=${PROJECT_SOURCE_DIR} make
                  WORKING_DIRECTORY ${TAPIR_APP_DIR})

add_custom_target(tapir-clean
                  COMMAND make clean
                  WORKING_DIRECTORY ${TAPIR_APP_DIR})

set(TAPIR_APP_DIR ${PROJECT_SOURCE_DIR}/apps/redis)
add_custom_target(redis
                  COMMAND ZEUS_SRC_DIR=${PROJECT_SOURCE_DIR} make
                  WORKING_DIRECTORY ${TAPIR_APP_DIR})


