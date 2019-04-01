file(GLOB ZEUS_COMMON_SOURCES "${CMAKE_CURRENT_SOURCE_DIR}/*.c" "${CMAKE_CURRENT_SOURCE_DIR}/*.cc")
add_library(dmtr-libos-rdmacm-common STATIC ${ZEUS_COMMON_SOURCES})
target_link_libraries(dmtr-libos-rdmacm-common rdmacm)
