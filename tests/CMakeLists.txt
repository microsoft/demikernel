# ---------------------------------------------------------
# Directory Source files
# ---------------------------------------------------------

# Zeus liblwip test code
file(GLOB SRC_ZEUS_LWIP_TEST "${PROJECT_SOURCE_DIR}/tests/*.cc")
set(SRC_ZEUS_LWIP_TEST ${SRC_ZEUS_LWIP_TEST} PARENT_SCOPE)
include_directories(${ZEUS_SOURCE_DIR}/liblwip)