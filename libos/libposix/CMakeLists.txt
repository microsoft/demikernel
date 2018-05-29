# ---------------------------------------------------------
# Directory Source files
# ---------------------------------------------------------

# Zeus lib os code
file(GLOB SRC_ZEUS_POSIX_LIB "${ZEUS_SOURCE_DIR}/libposix/*.cc")
set(SRC_ZEUS_POSIX_LIB ${SRC_ZEUS_POSIX_LIB} PARENT_SCOPE)
include_directories(${ZEUS_SOURCE_DIR}/libposix)
