# MTCP libos target
file(GLOB ZEUS_MTCP_SOURCES "${CMAKE_CURRENT_SOURCE_DIR}/*.cc")
add_library(zeus_mtcp STATIC ${ZEUS_COMMON_SOURCES} ${ZEUS_MTCP_SOURCES})
target_add_hoard(zeus_mtcp)
target_add_mtcp(zeus_mtcp)

