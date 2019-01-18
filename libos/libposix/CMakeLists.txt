# POSIX libos target
file(GLOB ZEUS_POSIX_SOURCES "${CMAKE_CURRENT_SOURCE_DIR}/*.cc")
# note: the libos library has to be a shared object in order to
# support the fact that we develop in C++ but need to also support
# applications written in C.
add_library(dmtr-libos-posix SHARED ${ZEUS_POSIX_SOURCES})
target_add_hoard(dmtr-libos-posix hoard-vanilla)

# the `--whole-archive` option is needed to ensure that symbols such as
# `zeus_queue` get exported from the resulting shared object. object
# files are normally culled if none of their symbols are referenced.
target_link_libraries(dmtr-libos-posix "-Wl,--whole-archive" dmtr-libos-common "-Wl,--no-whole-archive")
