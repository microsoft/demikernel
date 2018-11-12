include(ExternalProject)

# hoard
set(HEAPLAYERS_SOURCE_DIR ${CMAKE_SOURCE_DIR}/submodules/Heap-Layers)
function(target_add_hoard TARGET HOARD_SOURCE_DIR)
  set(HOARD_TARGET hoard-${TARGET})
  set(HOARD_BINARY_DIR ${CMAKE_BINARY_DIR}/ExternalProject/${HOARD_TARGET})
  set(HOARD_LIBS ${HOARD_SOURCE_DIR}/src/libhoard.so)
  ExternalProject_Add(${HOARD_TARGET}
    PREFIX ${HOARD_BINARY_DIR}
    SOURCE_DIR ${HOARD_SOURCE_DIR}
    CONFIGURE_COMMAND echo "No configure command for target `${HOARD_TARGET}`."
    BUILD_COMMAND HEAP_LAYERS=${HEAPLAYERS_SOURCE_DIR} make -C ${HOARD_SOURCE_DIR}/src
    INSTALL_COMMAND echo "No install command for target `${HOARD_TARGET}`."
  )
  target_link_libraries(${TARGET} ${HOARD_LIBS})
  target_include_directories(${TARGET} PUBLIC
    ${HOARD_SOURCE_DIR}/src/include
    ${HEAPLAYERS_SOURCE_DIR}
  )
  add_dependencies(${TARGET} ${HOARD_TARGET})
endfunction(target_add_hoard)
