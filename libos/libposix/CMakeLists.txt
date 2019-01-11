# POSIX libos target
file(GLOB ZEUS_POSIX_SOURCES "${CMAKE_CURRENT_SOURCE_DIR}/*.cc")
add_library(zeus_posix SHARED ${ZEUS_POSIX_SOURCES})

# the `--whole-archive` option is needed to ensure that symbols such as
# `zeus_queue` get exported from the resulting shared object. object
# files are normally culled if none of their symbols are referenced.
target_link_libraries(zeus_posix "-Wl,--whole-archive" zeus_common "-Wl,--no-whole-archive")
