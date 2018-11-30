include(ExternalProject)

include(dpdk)

# mtcp
set(MTCP_SOURCE_DIR ${CMAKE_CURRENT_SOURCE_DIR}/submodules/mtcp)
set(MTCP_BINARY_DIR ${CMAKE_CURRENT_BINARY_DIR}/ExternalProject/mtcp)
set(MTCP_LIBS ${MTCP_SOURCE_DIR}/mtcp/lib/libmtcp.a)
ExternalProject_Add(mtcp
  PREFIX ${MTCP_BINARY_DIR}
  DEPENDS dpdk
  SOURCE_DIR ${MTCP_SOURCE_DIR}
  CONFIGURE_COMMAND cd ${MTCP_SOURCE_DIR} &&  ./configure --with-dpdk-lib=${DPDK_SOURCE_DIR}/${DPDK_TARGET} CFLAGS=-I${CMAKE_CURRENT_SOURCE_DIR}
  BUILD_COMMAND make -C ${MTCP_SOURCE_DIR}
  INSTALL_COMMAND echo 'MTCP doesn't support an install step'
)
function(target_add_mtcp TARGET)
  target_link_libraries(${TARGET} ${MTCP_LIBS})
  target_include_directories(${TARGET} PUBLIC ${MTCP_SOURCE_DIR}/mtcp/include)
  add_dependencies(${TARGET} mtcp)
  # note: this must follow `target_link_libraries()` or link errors
  # will manifest!
  target_add_dpdk(${TARGET})
endfunction(target_add_mtcp)
