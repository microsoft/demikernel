# ---------------------------------------------------------
# Directory Source files
# ---------------------------------------------------------

# Zeus lib os code
file(GLOB SRC_ZEUS_COMMON_LIB "${ZEUS_SOURCE_DIR}/common/*.cc")
set(SRC_ZEUS_COMMON_LIB ${SRC_ZEUS_COMMON_LIB} PARENT_SCOPE)
include_directories(${ZEUS_SOURCE_DIR}/common)
