# LWIP libos target
file(GLOB ZEUS_LIBLWIP_SOURCES ${CMAKE_CURRENT_SOURCE_DIR}/*.cc)
# note: the libos library has to be a shared object in order to
# support the fact that we develop in C++ but need to also support
# applications written in C.
add_library(zeus_lwip SHARED ${ZEUS_LIBLWIP_SOURCES})
target_add_hoard(zeus_lwip ${CMAKE_SOURCE_DIR}/submodules/Hoard)
target_add_dpdk(zeus_lwip)
target_include_directories(zeus_lwip PUBLIC ${CMAKE_CURRENT_SOURCE_DIR}/liblwip ${DPDK_INCLUDE_DIR} )

target_link_libraries(zeus_lwip Threads::Threads)

# the `--whole-archive` option is needed to ensure that symbols such as
# `zeus_queue` get exported from the resulting shared object. object
# files are normally culled if none of their symbols are referenced.
target_link_libraries(zeus_lwip "-Wl,--whole-archive" zeus_common "-Wl,--no-whole-archive")

# todo: these options should not be passed directly to
# `target_link_libraries()`. we should use `find_library()` instead.
target_link_libraries(zeus_lwip "-Wl,--whole-archive -lnuma -Wl,--no-whole-archive")
