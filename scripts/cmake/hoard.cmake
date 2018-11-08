# hoard
set(HOARD_SOURCE_DIR ${CMAKE_SOURCE_DIR}/libos/librdma/submodules/Hoard)
set(HEAPLAYERS_SOURCE_DIR ${CMAKE_SOURCE_DIR}/submodules/Heap-Layers)
set(HOARD_LIBS ${HOARD_SOURCE_DIR}/src/libhoard.so)
# for some reason, CMake refused to recognize `OUTPUT`s specified with
# `add_custom_command()`, so i had to hack something together with
# `add_custom_target()` instead. :(
add_custom_target(hoard_rdma
  COMMAND HEAP_LAYERS=${HEAPLAYERS_SOURCE_DIR} make
  WORKING_DIRECTORY ${HOARD_SOURCE_DIR}/src
)
function(target_add_hoard TARGET)
  target_link_libraries(${TARGET} ${HOARD_LIBS} rdmacm)
  target_include_directories(${TARGET} PUBLIC
    ${HOARD_SOURCE_DIR}/src/include
    ${HEAPLAYERS_SOURCE_DIR}
  )
  add_dependencies(${TARGET} hoard_rdma)
endfunction(target_add_hoard)
