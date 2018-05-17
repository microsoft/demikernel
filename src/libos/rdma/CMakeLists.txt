# ---------------------------------------------------------
# Directory Source files
# ---------------------------------------------------------

# Zeus lib os code
file(GLOB SRC_ZEUS_RDMA_LIB "${ZEUS_SOURCE_DIR}/libos/rdma/*.cc")

set(SRC_ZEUS_RDMA_LIB ${SRC_ZEUS_RDMA_LIB} PARENT_SCOPE)

include_directories(${ZEUS_SOURCE_DIR}/libos/rdma)
