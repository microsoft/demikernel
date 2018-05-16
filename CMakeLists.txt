# Zeus libOS code

cmake_minimum_required (VERSION 3.8.2)
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
  -Wno-uninitialized -O3 -DNASSERT -lrdmacm -libverbs")
set(CMAKE_CXX_FLAGS "${CMAKE_CXX_FLAGS} -g -fstack-protector \
   -std=c++11 -fPIC -Wall -lrdmacm -libverbs")
   set(CMAKE_LD_FLAGS "${CMAKE_LD_FLAGS}")

# ---------------------------------------------------------
# Include and link directories
# ---------------------------------------------------------

set(ZEUS_SOURCE_DIR ${PROJECT_SOURCE_DIR}/src)
include_directories(${ZEUS_SOURCE_DIR})
#include_directories(${PROJECT_BINARY_DIR})

# Hoard

ExternalProject_Add(Hoard
  PREFIX ${PROJECT_SOURCE_DIR}
  GIT_REPOSITORY git@github.com:iyzhang/Hoard.git
  CONFIGURE_COMMAND ""
  UPDATE_DISCONNECTED 1
  BUILD_COMMAND "make"
  SOURCE_DIR ${PROJECT_SOURCE_DIR}/src/Hoard/src/src
  BINARY_DIR ${PROJECT_SOURCE_DIR}/src/Hoard/src/src
  INSTALL_COMMAND ""
  )

set(HOARD_LIBRARY ${PROJECT_SOURCE_DIR}/Hoard/src})

# ---------------------------------------------------------
# Source files
# ---------------------------------------------------------

add_subdirectory(src/libos)

# ---------------------------------------------------------
# Build targets
# ---------------------------------------------------------

add_library(zeus SHARED
  ${SRC_ZEUS_LIB})
target_link_libraries(zeus ${HOARD_LIBRARIES})
