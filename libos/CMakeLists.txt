add_subdirectory(${CMAKE_CURRENT_SOURCE_DIR}/common)
add_subdirectory(${CMAKE_CURRENT_SOURCE_DIR}/posix)
add_subdirectory(${CMAKE_CURRENT_SOURCE_DIR}/lwip)
add_subdirectory(${CMAKE_CURRENT_SOURCE_DIR}/rdma)

#add_subdirectory(${CMAKE_CURRENT_SOURCE_DIR}/libspdk)

if(BUILD_MTCP AND NOT AZURE_SUPPORT)
    add_subdirectory(${CMAKE_CURRENT_SOURCE_DIR}/libmtcp)
endif(BUILD_MTCP AND NOT AZURE_SUPPORT)
