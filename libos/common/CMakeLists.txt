file(GLOB ZEUS_COMMON_SOURCES "${CMAKE_CURRENT_SOURCE_DIR}/*.c" "${CMAKE_CURRENT_SOURCE_DIR}/*.cc")
add_library(dmtr-libos-common STATIC ${ZEUS_COMMON_SOURCES})
target_add_plf(dmtr-libos-common)
target_link_libraries(dmtr-libos-common rdmacm)