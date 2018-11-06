# spdk libos target
file(GLOB ZEUS_SPDK_LIB "${CMAKE_CURRENT_SOURCE_DIR}/*.cc")
add_library(zeus_spdk STATIC ${ZEUS_COMMON_SOURCES} ${ZEUS_SPDK_LIB} ${ZEUS_POSIX_SOURCES})
target_add_hoard(zeus_spdk)
target_add_spdk(zeus_spdk)
