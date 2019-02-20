# LWIP libos target
file(GLOB ZEUS_LIBLWIP_SOURCES ${CMAKE_CURRENT_SOURCE_DIR}/*.cc)
# note: the libos library has to be a shared object in order to
# support the fact that we develop in C++ but need to also support
# applications written in C.
add_library(dmtr-libos-lwip SHARED ${ZEUS_LIBLWIP_SOURCES})
target_add_hoard(dmtr-libos-lwip hoard-vanilla)
target_add_dpdk(dmtr-libos-lwip)
target_include_directories(dmtr-libos-lwip PUBLIC ${CMAKE_CURRENT_SOURCE_DIR}/liblwip ${DPDK_INCLUDE_DIR} )

target_link_libraries(dmtr-libos-lwip Threads::Threads)

# the `--whole-archive` option is needed to ensure that symbols such as
# `zeus_queue` get exported from the resulting shared object. object
# files are normally culled if none of their symbols are referenced.
target_link_libraries(dmtr-libos-lwip "-Wl,--whole-archive" dmtr-libos-common "-Wl,--no-whole-archive")

# todo: these options should not be passed directly to
# `target_link_libraries()`. we should use `find_library()` instead.
target_link_libraries(dmtr-libos-lwip "-Wl,--whole-archive -lnuma -Wl,--no-whole-archive")
