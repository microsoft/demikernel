include(ExternalProject)

# dpdk
# note: we have no choice but to use a customized version of dpdk provided
# by mtcp.
set(DPDK_TARGET x86_64-native-linuxapp-gcc CACHE STRING "The DPDK Target")
set(DPDK_SOURCE_DIR ${CMAKE_CURRENT_SOURCE_DIR}/submodules/mtcp/dpdk-17.08)
set(DPDK_BINARY_DIR ${CMAKE_CURRENT_BINARY_DIR}/submodules/dpdk)
set(DPDK_INSTALL_DIR ${DPDK_SOURCE_DIR}/${DPDK_TARGET})
set(DPDK_INCLUDE_DIR ${DPDK_INSTALL_DIR}/include)
set(DPDK_LIB_DIR ${DPDK_INSTALL_DIR}/lib)
file(MAKE_DIRECTORY ${DPDK_INCLUDE_DIR})
file(MAKE_DIRECTORY ${DPDK_LIB_DIR})
set(DPDK_LIBS
  ${DPDK_LIB_DIR}/libdpdk.a
  ${DPDK_LIB_DIR}/librte_acl.a
  ${DPDK_LIB_DIR}/librte_bitratestats.a
  ${DPDK_LIB_DIR}/librte_cfgfile.a
  ${DPDK_LIB_DIR}/librte_cmdline.a
  ${DPDK_LIB_DIR}/librte_cryptodev.a
  ${DPDK_LIB_DIR}/librte_distributor.a
  ${DPDK_LIB_DIR}/librte_eal.a
  ${DPDK_LIB_DIR}/librte_efd.a
  ${DPDK_LIB_DIR}/librte_ethdev.a
  ${DPDK_LIB_DIR}/librte_eventdev.a
  ${DPDK_LIB_DIR}/librte_gro.a
  ${DPDK_LIB_DIR}/librte_hash.a
  ${DPDK_LIB_DIR}/librte_ip_frag.a
  ${DPDK_LIB_DIR}/librte_jobstats.a
  ${DPDK_LIB_DIR}/librte_kni.a
  ${DPDK_LIB_DIR}/librte_kvargs.a
  ${DPDK_LIB_DIR}/librte_latencystats.a
  ${DPDK_LIB_DIR}/librte_lpm.a
  ${DPDK_LIB_DIR}/librte_mbuf.a
  ${DPDK_LIB_DIR}/librte_mempool.a
  ${DPDK_LIB_DIR}/librte_mempool_ring.a
  ${DPDK_LIB_DIR}/librte_mempool_stack.a
  ${DPDK_LIB_DIR}/librte_meter.a
  ${DPDK_LIB_DIR}/librte_metrics.a
  ${DPDK_LIB_DIR}/librte_net.a
  ${DPDK_LIB_DIR}/librte_pdump.a
  ${DPDK_LIB_DIR}/librte_pipeline.a
  ${DPDK_LIB_DIR}/librte_pmd_af_packet.a
  ${DPDK_LIB_DIR}/librte_pmd_ark.a
  ${DPDK_LIB_DIR}/librte_pmd_avp.a
  ${DPDK_LIB_DIR}/librte_pmd_bnxt.a
  ${DPDK_LIB_DIR}/librte_pmd_bond.a
  ${DPDK_LIB_DIR}/librte_pmd_crypto_scheduler.a
  ${DPDK_LIB_DIR}/librte_pmd_cxgbe.a
  ${DPDK_LIB_DIR}/librte_pmd_e1000.a
  ${DPDK_LIB_DIR}/librte_pmd_ena.a
  ${DPDK_LIB_DIR}/librte_pmd_enic.a
  ${DPDK_LIB_DIR}/librte_pmd_failsafe.a
  ${DPDK_LIB_DIR}/librte_pmd_fm10k.a
  ${DPDK_LIB_DIR}/librte_pmd_i40e.a
  ${DPDK_LIB_DIR}/librte_pmd_ixgbe.a
  ${DPDK_LIB_DIR}/librte_pmd_kni.a
  ${DPDK_LIB_DIR}/librte_pmd_lio.a
  ${DPDK_LIB_DIR}/librte_pmd_nfp.a
  ${DPDK_LIB_DIR}/librte_pmd_null.a
  ${DPDK_LIB_DIR}/librte_pmd_null_crypto.a
  ${DPDK_LIB_DIR}/librte_pmd_octeontx_ssovf.a
  ${DPDK_LIB_DIR}/librte_pmd_qede.a
  ${DPDK_LIB_DIR}/librte_pmd_ring.a
  ${DPDK_LIB_DIR}/librte_pmd_sfc_efx.a
  ${DPDK_LIB_DIR}/librte_pmd_skeleton_event.a
  ${DPDK_LIB_DIR}/librte_pmd_sw_event.a
  ${DPDK_LIB_DIR}/librte_pmd_tap.a
  ${DPDK_LIB_DIR}/librte_pmd_thunderx_nicvf.a
  ${DPDK_LIB_DIR}/librte_pmd_vhost.a
  ${DPDK_LIB_DIR}/librte_pmd_virtio.a
  ${DPDK_LIB_DIR}/librte_pmd_vmxnet3_uio.a
  ${DPDK_LIB_DIR}/librte_port.a
  ${DPDK_LIB_DIR}/librte_power.a
  ${DPDK_LIB_DIR}/librte_reorder.a
  ${DPDK_LIB_DIR}/librte_ring.a
  ${DPDK_LIB_DIR}/librte_sched.a
  ${DPDK_LIB_DIR}/librte_table.a
  ${DPDK_LIB_DIR}/librte_timer.a
  ${DPDK_LIB_DIR}/librte_vhost.a
)
# ExternalProject_Add(dpdk
#   PREFIX ${DPDK_BINARY_DIR}/ExternalProject
#   SOURCE_DIR ${DPDK_SOURCE_DIR}
#   CONFIGURE_COMMAND make -C ${DPDK_SOURCE_DIR} config  T=${DPDK_TARGET} DESTDIR=${DPDK_BINARY_DIR}
#   BUILD_COMMAND make -C ${DPDK_SOURCE_DIR} T=${DPDK_TARGET}
#   INSTALL_COMMAND make -C ${DPDK_SOURCE_DIR} install T=${DPDK_TARGET} DESTDIR=${DPDK_BINARY_DIR}
# )
ExternalProject_Add(dpdk
  PREFIX ${DPDK_BINARY_DIR}/ExternalProject
  SOURCE_DIR ${DPDK_SOURCE_DIR}
  CONFIGURE_COMMAND make -C ${DPDK_SOURCE_DIR} config  T=${DPDK_TARGET} DESTDIR=${DPDK_SOURCE_DIR}
  BUILD_COMMAND make -C ${DPDK_SOURCE_DIR} T=${DPDK_TARGET}
  INSTALL_COMMAND make -C ${DPDK_SOURCE_DIR} install T=${DPDK_TARGET} DESTDIR=${DPDK_SOURCE_DIR}
)
function(target_add_dpdk TARGET)
  target_link_libraries(${TARGET} ${DPDK_LIBS})
  target_include_directories(${TARGET} PUBLIC ${DPDK_INCLUDE_DIR})
  set_target_properties(${TARGET} PROPERTIES
    COMPILE_FLAGS "-march=native -DRTE_MACHINE_CPUFLAG_SSE -DRTE_MACHINE_CPUFLAG_SSE2 -DRTE_MACHINE_CPUFLAG_SSE3 -DRTE_MACHINE_CPUFLAG_SSSE3 -DRTE_MACHINE_CPUFLAG_SSE4_1 -DRTE_MACHINE_CPUFLAG_SSE4_2"
    LINK_FLAGS "-lrt -lm -lnuma -ldl -libverbs"
  )
  add_dependencies(${TARGET} dpdk)
endfunction(target_add_dpdk)

# mtcp
set(MTCP_SOURCE_DIR ${CMAKE_CURRENT_SOURCE_DIR}/submodules/mtcp)
set(MTCP_BINARY_DIR ${CMAKE_CURRENT_BINARY_DIR}/submodules/mtcp)
set(MTCP_LIBS ${MTCP_SOURCE_DIR}/mtcp/lib/libmtcp.a)
ExternalProject_Add(mtcp
  PREFIX ${MTCP_BINARY_DIR}/ExternalProject
  DEPENDS dpdk
  SOURCE_DIR ${MTCP_SOURCE_DIR}
  CONFIGURE_COMMAND cd ${MTCP_SOURCE_DIR} &&  ./configure --with-dpdk-lib=${DPDK_INSTALL_DIR} CFLAGS=-I${CMAKE_CURRENT_SOURCE_DIR}
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
