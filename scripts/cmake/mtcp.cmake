# dpdk
set(DPDK_TARGET x86_64-native-linuxapp-gcc CACHE STRING "The DPDK Target")
# note: we have no choice but to use a customized version of dpdk provided
# by mtcp.
set(DPDK_SOURCE_DIR ${CMAKE_SOURCE_DIR}/submodules/mtcp/dpdk-17.08)
set(DPDK_LIBS
  ${DPDK_SOURCE_DIR}/${DPDK_TARGET}/lib/libdpdk.a
  ${DPDK_SOURCE_DIR}/${DPDK_TARGET}/lib/librte_acl.a
  ${DPDK_SOURCE_DIR}/${DPDK_TARGET}/lib/librte_bitratestats.a
  ${DPDK_SOURCE_DIR}/${DPDK_TARGET}/lib/librte_cfgfile.a
  ${DPDK_SOURCE_DIR}/${DPDK_TARGET}/lib/librte_cmdline.a
  ${DPDK_SOURCE_DIR}/${DPDK_TARGET}/lib/librte_cryptodev.a
  ${DPDK_SOURCE_DIR}/${DPDK_TARGET}/lib/librte_distributor.a
  ${DPDK_SOURCE_DIR}/${DPDK_TARGET}/lib/librte_eal.a
  ${DPDK_SOURCE_DIR}/${DPDK_TARGET}/lib/librte_efd.a
  ${DPDK_SOURCE_DIR}/${DPDK_TARGET}/lib/librte_ethdev.a
  ${DPDK_SOURCE_DIR}/${DPDK_TARGET}/lib/librte_eventdev.a
  ${DPDK_SOURCE_DIR}/${DPDK_TARGET}/lib/librte_gro.a
  ${DPDK_SOURCE_DIR}/${DPDK_TARGET}/lib/librte_hash.a
  ${DPDK_SOURCE_DIR}/${DPDK_TARGET}/lib/librte_ip_frag.a
  ${DPDK_SOURCE_DIR}/${DPDK_TARGET}/lib/librte_jobstats.a
  ${DPDK_SOURCE_DIR}/${DPDK_TARGET}/lib/librte_kni.a
  ${DPDK_SOURCE_DIR}/${DPDK_TARGET}/lib/librte_kvargs.a
  ${DPDK_SOURCE_DIR}/${DPDK_TARGET}/lib/librte_latencystats.a
  ${DPDK_SOURCE_DIR}/${DPDK_TARGET}/lib/librte_lpm.a
  ${DPDK_SOURCE_DIR}/${DPDK_TARGET}/lib/librte_mbuf.a
  ${DPDK_SOURCE_DIR}/${DPDK_TARGET}/lib/librte_mempool.a
  ${DPDK_SOURCE_DIR}/${DPDK_TARGET}/lib/librte_mempool_ring.a
  ${DPDK_SOURCE_DIR}/${DPDK_TARGET}/lib/librte_mempool_stack.a
  ${DPDK_SOURCE_DIR}/${DPDK_TARGET}/lib/librte_meter.a
  ${DPDK_SOURCE_DIR}/${DPDK_TARGET}/lib/librte_metrics.a
  ${DPDK_SOURCE_DIR}/${DPDK_TARGET}/lib/librte_net.a
  ${DPDK_SOURCE_DIR}/${DPDK_TARGET}/lib/librte_pdump.a
  ${DPDK_SOURCE_DIR}/${DPDK_TARGET}/lib/librte_pipeline.a
  ${DPDK_SOURCE_DIR}/${DPDK_TARGET}/lib/librte_pmd_af_packet.a
  ${DPDK_SOURCE_DIR}/${DPDK_TARGET}/lib/librte_pmd_ark.a
  ${DPDK_SOURCE_DIR}/${DPDK_TARGET}/lib/librte_pmd_avp.a
  ${DPDK_SOURCE_DIR}/${DPDK_TARGET}/lib/librte_pmd_bnxt.a
  ${DPDK_SOURCE_DIR}/${DPDK_TARGET}/lib/librte_pmd_bond.a
  ${DPDK_SOURCE_DIR}/${DPDK_TARGET}/lib/librte_pmd_crypto_scheduler.a
  ${DPDK_SOURCE_DIR}/${DPDK_TARGET}/lib/librte_pmd_cxgbe.a
  ${DPDK_SOURCE_DIR}/${DPDK_TARGET}/lib/librte_pmd_e1000.a
  ${DPDK_SOURCE_DIR}/${DPDK_TARGET}/lib/librte_pmd_ena.a
  ${DPDK_SOURCE_DIR}/${DPDK_TARGET}/lib/librte_pmd_enic.a
  ${DPDK_SOURCE_DIR}/${DPDK_TARGET}/lib/librte_pmd_failsafe.a
  ${DPDK_SOURCE_DIR}/${DPDK_TARGET}/lib/librte_pmd_fm10k.a
  ${DPDK_SOURCE_DIR}/${DPDK_TARGET}/lib/librte_pmd_i40e.a
  ${DPDK_SOURCE_DIR}/${DPDK_TARGET}/lib/librte_pmd_ixgbe.a
  ${DPDK_SOURCE_DIR}/${DPDK_TARGET}/lib/librte_pmd_kni.a
  ${DPDK_SOURCE_DIR}/${DPDK_TARGET}/lib/librte_pmd_lio.a
  ${DPDK_SOURCE_DIR}/${DPDK_TARGET}/lib/librte_pmd_nfp.a
  ${DPDK_SOURCE_DIR}/${DPDK_TARGET}/lib/librte_pmd_null.a
  ${DPDK_SOURCE_DIR}/${DPDK_TARGET}/lib/librte_pmd_null_crypto.a
  ${DPDK_SOURCE_DIR}/${DPDK_TARGET}/lib/librte_pmd_octeontx_ssovf.a
  ${DPDK_SOURCE_DIR}/${DPDK_TARGET}/lib/librte_pmd_qede.a
  ${DPDK_SOURCE_DIR}/${DPDK_TARGET}/lib/librte_pmd_ring.a
  ${DPDK_SOURCE_DIR}/${DPDK_TARGET}/lib/librte_pmd_sfc_efx.a
  ${DPDK_SOURCE_DIR}/${DPDK_TARGET}/lib/librte_pmd_skeleton_event.a
  ${DPDK_SOURCE_DIR}/${DPDK_TARGET}/lib/librte_pmd_sw_event.a
  ${DPDK_SOURCE_DIR}/${DPDK_TARGET}/lib/librte_pmd_tap.a
  ${DPDK_SOURCE_DIR}/${DPDK_TARGET}/lib/librte_pmd_thunderx_nicvf.a
  ${DPDK_SOURCE_DIR}/${DPDK_TARGET}/lib/librte_pmd_vhost.a
  ${DPDK_SOURCE_DIR}/${DPDK_TARGET}/lib/librte_pmd_virtio.a
  ${DPDK_SOURCE_DIR}/${DPDK_TARGET}/lib/librte_pmd_vmxnet3_uio.a
  ${DPDK_SOURCE_DIR}/${DPDK_TARGET}/lib/librte_port.a
  ${DPDK_SOURCE_DIR}/${DPDK_TARGET}/lib/librte_power.a
  ${DPDK_SOURCE_DIR}/${DPDK_TARGET}/lib/librte_reorder.a
  ${DPDK_SOURCE_DIR}/${DPDK_TARGET}/lib/librte_ring.a
  ${DPDK_SOURCE_DIR}/${DPDK_TARGET}/lib/librte_sched.a
  ${DPDK_SOURCE_DIR}/${DPDK_TARGET}/lib/librte_table.a
  ${DPDK_SOURCE_DIR}/${DPDK_TARGET}/lib/librte_timer.a
  ${DPDK_SOURCE_DIR}/${DPDK_TARGET}/lib/librte_vhost.a
)
file(STRINGS ${DPDK_SOURCE_DIR}/${DPDK_TARGET}/include/cflags.txt DPDK_CFLAGS)
file(STRINGS ${DPDK_SOURCE_DIR}/${DPDK_TARGET}/lib/ldflags.txt DPDK_LDFLAGS)
add_custom_command(
  OUTPUT
    ${DPDK_LIBS}
    ${DPDK_SOURCE_DIR}/${DPDK_TARGET}/include/cflags.txt
    ${DPDK_SOURCE_DIR}/${DPDK_TARGET}/lib/ldflags.txt
  WORKING_DIRECTORY ${DPDK_SOURCE_DIR}
  COMMAND make config T=${DPDK_TARGET}
  COMMAND make T=${DPDK_TARGET}
)
function(target_add_dpdk TARGET)
  target_link_libraries(${TARGET} ${DPDK_LIBS})
  target_include_directories(${TARGET} PUBLIC ${DPDK_SOURCE_DIR}/${DPDK_TARGET}/include)
  set_target_properties(${TARGET} PROPERTIES
    COMPILE_FLAGS "${DPDK_CFLAGS}"
    LINK_FLAGS "${DPDK_LDFLAGS} -libverbs"
  )
endfunction(target_add_dpdk)

# mtcp
set(MTCP_SOURCE_DIR ${CMAKE_SOURCE_DIR}/submodules/mtcp)
set(MTCP_LIBS ${MTCP_SOURCE_DIR}/mtcp/lib/libmtcp.a)
add_custom_command(
  OUTPUT ${MTCP_LIBS}
  WORKING_DIRECTORY ${MTCP_SOURCE_DIR}
  DEPENDS ${DPDK_LIBS}
  COMMAND ./configure --with-dpdk-lib=${DPDK_SOURCE_DIR}/${DPDK_TARGET} CFLAGS=-I${CMAKE_SOURCE_DIR}
  COMMAND make
)
function(target_add_mtcp TARGET)
  target_add_dpdk(${TARGET})
  target_link_libraries(${TARGET} ${MTCP_LIBS})
  target_include_directories(${TARGET} PUBLIC ${MTCP_SOURCE_DIR}/mtcp/include)
endfunction(target_add_mtcp)
