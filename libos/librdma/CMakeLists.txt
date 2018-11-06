# RDMA libos target
file(GLOB ZEUS_RDMA_SOURCES "${CMAKE_CURRENT_SOURCE_DIR}/*.cc")
add_library(zeus_rdma SHARED ${ZEUS_COMMON_SOURCES} ${ZEUS_RDMA_SOURCES})
target_add_hoard(zeus_rdma)
target_link_libraries(zeus_rdma rdmacm ibverbs)

