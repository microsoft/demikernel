# LWIP libos target
file(GLOB ZEUS_LIBLWIP_SOURCES ${CMAKE_CURRENT_SOURCE_DIR}/*.cc)
add_library(zeus_lwip STATIC ${ZEUS_LIBLWIP_SOURCES})
target_add_hoard(zeus_lwip ${CMAKE_SOURCE_DIR}/submodules/Hoard)
target_add_dpdk(zeus_lwip)
target_include_directories(zeus_lwip PUBLIC ${CMAKE_CURRENT_SOURCE_DIR}/liblwip ${DPDK_INCLUDE_DIR} )
target_link_libraries(zeus_lwip zeus_common Threads::Threads)
# todo: these options should not be passed directly to
# `target_link_libraries()`. we should use `find_library()` instead.
target_link_libraries(zeus_lwip "-Wl,--whole-archive -lnuma -Wl,--no-whole-archive")
