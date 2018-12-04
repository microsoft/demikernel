add_subdirectory(${CMAKE_CURRENT_SOURCE_DIR}/common)
add_subdirectory(${CMAKE_CURRENT_SOURCE_DIR}/libposix)
add_subdirectory(${CMAKE_CURRENT_SOURCE_DIR}/liblwip)
add_subdirectory(${CMAKE_CURRENT_SOURCE_DIR}/librdma)
#add_subdirectory(${CMAKE_CURRENT_SOURCE_DIR}/libspdk)

if(MTCP_BUILD)
    add_subdirectory(${CMAKE_CURRENT_SOURCE_DIR}/libmtcp)
endif(MTCP_BUILD)
