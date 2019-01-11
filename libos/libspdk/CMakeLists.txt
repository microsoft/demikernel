# spdk libos target
file(GLOB ZEUS_SPDK_LIB "${CMAKE_CURRENT_SOURCE_DIR}/*.cc")
add_library(zeus_spdk STATIC ${ZEUS_SPDK_LIB})
target_add_hoard(zeus_spdk ${CMAKE_SOURCE_DIR}/submodules/Hoard)
target_add_spdk(zeus_spdk)

# the `--whole-archive` option is needed to ensure that symbols such as
# `zeus_queue` get exported from the resulting shared object. object
# files are normally culled if none of their symbols are referenced.
target_link_libraries(zeus_spdk "-Wl,--whole-archive" zeus_posix "-Wl,--no-whole-archive")
