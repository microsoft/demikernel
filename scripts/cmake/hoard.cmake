if(NOT HOARD_DOT_CMAKE_INCLUDED)
set(HOARD_DOT_CMAKE_INCLUDED YES)

include(ExternalProject)

set(HEAPLAYERS_SOURCE_DIR ${CMAKE_SOURCE_DIR}/submodules/Heap-Layers)

function(add_hoard HOARD_TARGET HOARD_SOURCE_DIR)
  set(HOARD_BINARY_DIR ${CMAKE_BINARY_DIR}/ExternalProject/${HOARD_TARGET})
  if(CMAKE_BUILD_TYPE MATCHES "Rel")
    set(HOARD_CPPFLAGS "CPPFLAGS_EXTRA=-O3 -DNDEBUG")
  else(CMAKE_BUILD_TYPE MATCHES "Rel")
    set(HOARD_CPPFLAGS "CPPFLAGS_EXTRA=-g -O0")
  endif(CMAKE_BUILD_TYPE MATCHES "Rel")
  ExternalProject_Add(${HOARD_TARGET}
    PREFIX ${HOARD_BINARY_DIR}
    SOURCE_DIR ${HOARD_SOURCE_DIR}
    CONFIGURE_COMMAND echo "No configure command for target `${HOARD_TARGET}`."
    BUILD_COMMAND HEAP_LAYERS=${HEAPLAYERS_SOURCE_DIR} make -C ${HOARD_SOURCE_DIR}/src
    INSTALL_COMMAND echo "No install command for target `${HOARD_TARGET}`."
  )
endfunction(add_hoard)

function(target_add_hoard TARGET HOARD_TARGET)
  ExternalProject_Get_Property(${HOARD_TARGET} SOURCE_DIR)
  set(HOARD_LIBS ${SOURCE_DIR}/src/libhoard.so)
  target_link_libraries(${TARGET} ${HOARD_LIBS})
  target_include_directories(${TARGET} PUBLIC
    ${SOURCE_DIR}/src/include
    ${HEAPLAYERS_SOURCE_DIR}
  )
  add_dependencies(${TARGET} ${HOARD_TARGET})
endfunction(target_add_hoard)

add_hoard(hoard-vanilla ${CMAKE_SOURCE_DIR}/submodules/Hoard)
add_hoard(hoard-rdma ${CMAKE_SOURCE_DIR}/submodules/HoardRdma)

endif(NOT HOARD_DOT_CMAKE_INCLUDED)
