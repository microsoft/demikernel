if(NOT MTCP_DOT_CMAKE_INCLUDED)
set(MTCP_DOT_CMAKE_INCLUDED YES)

option(MTCP_BUILD "include mTCP in build (doesn't work within Azure)" ON)
if(MTCP_BUILD)

include(ExternalProject)
include(dpdk) # we need access to DPDK-related cache variables

# mTCP's fork of DPDK
set(MTCP_DPDK_SOURCE_DIR ${CMAKE_SOURCE_DIR}/submodules/mtcp/dpdk-17.08)
set(MTCP_DPDK_BINARY_DIR ${CMAKE_BINARY_DIR}/ExternalProject/mtcp_dpdk)
set(MTCP_DPDK_INSTALL_DIR ${MTCP_DPDK_BINARY_DIR})
set(MTCP_DPDK_INCLUDE_DIR ${MTCP_DPDK_INSTALL_DIR}/include/dpdk)
set(MTCP_DPDK_LIB_DIR ${MTCP_DPDK_INSTALL_DIR}/lib)
file(MAKE_DIRECTORY ${MTCP_DPDK_SOURCE_DIR}/${DPDK_TARGET}/include)
file(MAKE_DIRECTORY ${MTCP_DPDK_SOURCE_DIR}/${DPDK_TARGET}/lib)

ExternalProject_Add(mtcp_dpdk
  PREFIX ${MTCP_DPDK_BINARY_DIR}
  SOURCE_DIR ${MTCP_DPDK_SOURCE_DIR}
  CONFIGURE_COMMAND make -C ${MTCP_DPDK_SOURCE_DIR} config  T=${DPDK_TARGET}
  BUILD_COMMAND make -C ${MTCP_DPDK_SOURCE_DIR} T=${DPDK_TARGET}
  INSTALL_COMMAND make -C ${MTCP_DPDK_SOURCE_DIR} install T=${DPDK_TARGET} DESTDIR=${MTCP_DPDK_INSTALL_DIR}
)

if(DPDK_USE_MELLANOX_PMD)
  set(MTCP_DPDK_CONFIG_RTE_LIBRTR_MLX5_PMD y)
else(DPDK_USE_MELLANOX_PMD)
  set(MTCP_DPDK_CONFIG_RTE_LIBRTR_MLX5_PMD n)
endif(DPDK_USE_MELLANOX_PMD)
set(MTCP_DPDK_CONFIG_COMMON_BASE ${MTCP_DPDK_SOURCE_DIR}/config/common_base)
configure_file(${MTCP_DPDK_CONFIG_COMMON_BASE}.in ${MTCP_DPDK_CONFIG_COMMON_BASE})

# mTCP has hacked the DPDK build to divulge the flags it generated for
# compilation and linking.
set(MTCP_DPDK_CFLAGS_FILE ${MTCP_DPDK_SOURCE_DIR}/${DPDK_TARGET}/include/cflags.txt)
set(MTCP_DPDK_LDFLAGS_FILE ${MTCP_DPDK_SOURCE_DIR}/${DPDK_TARGET}/lib/ldflags.txt)

# mTCP
set(MTCP_SOURCE_DIR ${CMAKE_CURRENT_SOURCE_DIR}/submodules/mtcp)
set(MTCP_BINARY_DIR ${CMAKE_CURRENT_BINARY_DIR}/ExternalProject/mtcp)
set(MTCP_LIBS ${MTCP_SOURCE_DIR}/mtcp/lib/libmtcp.a)
ExternalProject_Add(mtcp
  PREFIX ${MTCP_BINARY_DIR}
  DEPENDS mtcp_dpdk
  SOURCE_DIR ${MTCP_SOURCE_DIR}
  CONFIGURE_COMMAND cd ${MTCP_SOURCE_DIR} &&  ./configure --with-dpdk-lib=${MTCP_DPDK_SOURCE_DIR}/${DPDK_TARGET} CFLAGS=-I${CMAKE_CURRENT_SOURCE_DIR}
  BUILD_COMMAND make -C ${MTCP_SOURCE_DIR}
  INSTALL_COMMAND echo 'mTCP doesn't support an install step'
)
function(target_add_mtcp TARGET)
  target_link_libraries(${TARGET} ${MTCP_LIBS})
  target_include_directories(${TARGET} PUBLIC ${MTCP_SOURCE_DIR}/mtcp/include ${MTCP_DPDK_INCLUDE_DIR})
  add_dependencies(${TARGET} mtcp mtcp_dpdk)
  # mTCP's DPDK flags
  set_target_properties(${TARGET} PROPERTIES
    COMPILE_FLAGS @${MTCP_DPDK_CFLAGS_FILE}
    LINK_FLAGS @${MTCP_DPDK_LDFLAGS_FILE}
  )
endfunction(target_add_mtcp)

endif(MTCP_BUILD)
endif(NOT MTCP_DOT_CMAKE_INCLUDED)
