# POSIX libos target
file(GLOB ZEUS_POSIX_SOURCES "${CMAKE_CURRENT_SOURCE_DIR}/*.cc")
add_library(zeus_posix SHARED ${ZEUS_POSIX_SOURCES})
target_link_libraries(zeus_posix zeus_common)
