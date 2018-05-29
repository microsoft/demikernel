# ---------------------------------------------------------
# Directory Source files
# ---------------------------------------------------------

# Zeus lib os code
file(GLOB SRC_ZEUS_DPDK_LIB "${ZEUS_SOURCE_DIR}/libos/dpdk/*.cc")

set(SRC_ZEUS_DPDK_LIB ${SRC_ZEUS_DPDK_LIB} PARENT_SCOPE)

include_directories(${ZEUS_SOURCE_DIR}/libos/dpdk)
