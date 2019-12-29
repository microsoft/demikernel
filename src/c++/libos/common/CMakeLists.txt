file(GLOB ZEUS_COMMON_SOURCES "${CMAKE_CURRENT_SOURCE_DIR}/*.c" "${CMAKE_CURRENT_SOURCE_DIR}/*.cc")
add_library(psp-libos-common STATIC ${ZEUS_COMMON_SOURCES})
target_link_libraries(psp-libos-common dmtr-libos-io-common yaml-cpp)
