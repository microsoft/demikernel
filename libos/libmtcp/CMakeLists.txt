# ---------------------------------------------------------
# Directory Source files
# ---------------------------------------------------------

# Zeus lib os code
file(GLOB SRC_ZEUS_MTCP_LIB "${ZEUS_SOURCE_DIR}/libmtcp/*.cc")
set(SRC_ZEUS_MTCP_LIB ${SRC_ZEUS_MTCP_LIB} PARENT_SCOPE)
include_directories(${ZEUS_SOURCE_DIR}/libmtcp)
