# spdk libos target
file(GLOB ZEUS_SPDK_LIB "${CMAKE_CURRENT_SOURCE_DIR}/*.cc")
add_library(zeus_spdk STATIC ${ZEUS_SPDK_LIB})
target_add_hoard(zeus_spdk ${CMAKE_SOURCE_DIR}/submodules/Hoard)
target_add_spdk(zeus_spdk)
target_link_libraries(zeus_spdk zeus_common zeus_posix)
