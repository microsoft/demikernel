# ---------------------------------------------------------
# Directory Source files
# ---------------------------------------------------------

# Zeus lib os code
file(GLOB SRC_ZEUS_SPDK_LIB "${ZEUS_SOURCE_DIR}/libspdk/*.cc")
set(SRC_ZEUS_SPDK_LIB ${SRC_ZEUS_SPDK_LIB} PARENT_SCOPE)
include_directories(${ZEUS_SOURCE_DIR}/libspdk)
