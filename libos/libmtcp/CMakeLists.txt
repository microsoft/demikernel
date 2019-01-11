# MTCP libos target
file(GLOB ZEUS_MTCP_SOURCES "${CMAKE_CURRENT_SOURCE_DIR}/*.cc")
add_library(zeus_mtcp STATIC ${ZEUS_MTCP_SOURCES})
target_add_hoard(zeus_mtcp ${CMAKE_SOURCE_DIR}/submodules/Hoard)
target_add_mtcp(zeus_mtcp)

# the `--whole-archive` option is needed to ensure that symbols such as
# `zeus_queue` get exported from the resulting shared object. object
# files are normally culled if none of their symbols are referenced.
target_link_libraries(zeus_mtcp "-Wl,--whole-archive" zeus_common "-Wl,--no-whole-archive")
