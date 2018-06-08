# ---------------------------------------------------------
# Directory Source files
# ---------------------------------------------------------

# Zeus lib os code
file(GLOB SRC_ZEUS_LWIP_LIB "${ZEUS_SOURCE_DIR}/liblwip/*.cc")
set(SRC_ZEUS_LWIP_LIB ${SRC_ZEUS_LWIP_LIB} PARENT_SCOPE)
include_directories(${ZEUS_SOURCE_DIR}/liblwip)
