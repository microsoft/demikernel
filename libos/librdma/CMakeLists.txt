# RDMA libos target
file(GLOB ZEUS_RDMA_SOURCES "${CMAKE_CURRENT_SOURCE_DIR}/*.cc")
add_library(zeus_rdma SHARED ${ZEUS_RDMA_SOURCES})
target_add_hoard(zeus_rdma ${CMAKE_SOURCE_DIR}/submodules/HoardRdma)
target_link_libraries(zeus_rdma zeus_common rdmacm ibverbs)
