# Copyright (c) Microsoft Corporation.
# Licensed under the MIT license.

if(NOT MIMALLOC_DOT_CMAKE_INCLUDED)
set(MIMALLOC_DOT_CMAKE_INCLUDED YES)

include(ExternalProject)

set(MIMALLOC_SOURCE_DIR ${CMAKE_SOURCE_DIR}/submodules/mimalloc)
set(MIMALLOC_BINARY_DIR ${CMAKE_BINARY_DIR}/ExternalProject/mimalloc)

function(target_add_mimalloc TARGET)
  ExternalProject_Add(mimalloc
    PREFIX ${MIMALLOC_BINARY_DIR}
    SOURCE_DIR ${MIMALLOC_SOURCE_DIR}
    BINARY_DIR ${MIMALLOC_BINARY_DIR}
    CONFIGURE_COMMAND cmake ${MIMALLOC_SOURCE_DIR}
    BUILD_COMMAND make
    INSTALL_COMMAND echo "No install command for target `${HOARD_TARGET}`."
  )

  set(MIMALLOC_LIBS ${MIMALLOC_BINARY_DIR}/libmimalloc.so)
  target_link_libraries(${TARGET} ${MIMALLOC_LIBS})
  target_include_directories(${TARGET} PUBLIC
    ${SOURCE_DIR}/src/include
  )
  add_dependencies(${TARGET} mimalloc)
  set_property(
    TARGET ${TARGET}
    PROPERTY MIMALLOC ${MIMALLOC_TARGET}
  )
endfunction(target_add_mimalloc)

endif(NOT MIMALLOC_DOT_CMAKE_INCLUDED)
