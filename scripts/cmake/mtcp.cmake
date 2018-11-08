# dpdk
set(DPDK_TARGET x86_64-native-linuxapp-gcc CACHE STRING "The DPDK Target")
# note: we have no choice but to use a customized version of dpdk provided
# by mtcp.
set(DPDK_SOURCE_DIR ${CMAKE_SOURCE_DIR}/submodules/mtcp/dpdk-17.08)
set(DPDK_BINARY_DIR ${CMAKE_BINARY_DIR}/submodules/dpdk)
set(DPDK_LIBS
  ${DPDK_BINARY_DIR}/lib/libdpdk.a
  ${DPDK_BINARY_DIR}/lib/librte_acl.a
  ${DPDK_BINARY_DIR}/lib/librte_bitratestats.a
  ${DPDK_BINARY_DIR}/lib/librte_cfgfile.a
  ${DPDK_BINARY_DIR}/lib/librte_cmdline.a
  ${DPDK_BINARY_DIR}/lib/librte_cryptodev.a
  ${DPDK_BINARY_DIR}/lib/librte_distributor.a
  ${DPDK_BINARY_DIR}/lib/librte_eal.a
  ${DPDK_BINARY_DIR}/lib/librte_efd.a
  ${DPDK_BINARY_DIR}/lib/librte_ethdev.a
  ${DPDK_BINARY_DIR}/lib/librte_eventdev.a
  ${DPDK_BINARY_DIR}/lib/librte_gro.a
  ${DPDK_BINARY_DIR}/lib/librte_hash.a
  ${DPDK_BINARY_DIR}/lib/librte_ip_frag.a
  ${DPDK_BINARY_DIR}/lib/librte_jobstats.a
  ${DPDK_BINARY_DIR}/lib/librte_kni.a
  ${DPDK_BINARY_DIR}/lib/librte_kvargs.a
  ${DPDK_BINARY_DIR}/lib/librte_latencystats.a
  ${DPDK_BINARY_DIR}/lib/librte_lpm.a
  ${DPDK_BINARY_DIR}/lib/librte_mbuf.a
  ${DPDK_BINARY_DIR}/lib/librte_mempool.a
  ${DPDK_BINARY_DIR}/lib/librte_mempool_ring.a
  ${DPDK_BINARY_DIR}/lib/librte_mempool_stack.a
  ${DPDK_BINARY_DIR}/lib/librte_meter.a
  ${DPDK_BINARY_DIR}/lib/librte_metrics.a
  ${DPDK_BINARY_DIR}/lib/librte_net.a
  ${DPDK_BINARY_DIR}/lib/librte_pdump.a
  ${DPDK_BINARY_DIR}/lib/librte_pipeline.a
  ${DPDK_BINARY_DIR}/lib/librte_pmd_af_packet.a
  ${DPDK_BINARY_DIR}/lib/librte_pmd_ark.a
  ${DPDK_BINARY_DIR}/lib/librte_pmd_avp.a
  ${DPDK_BINARY_DIR}/lib/librte_pmd_bnxt.a
  ${DPDK_BINARY_DIR}/lib/librte_pmd_bond.a
  ${DPDK_BINARY_DIR}/lib/librte_pmd_crypto_scheduler.a
  ${DPDK_BINARY_DIR}/lib/librte_pmd_cxgbe.a
  ${DPDK_BINARY_DIR}/lib/librte_pmd_e1000.a
  ${DPDK_BINARY_DIR}/lib/librte_pmd_ena.a
  ${DPDK_BINARY_DIR}/lib/librte_pmd_enic.a
  ${DPDK_BINARY_DIR}/lib/librte_pmd_failsafe.a
  ${DPDK_BINARY_DIR}/lib/librte_pmd_fm10k.a
  ${DPDK_BINARY_DIR}/lib/librte_pmd_i40e.a
  ${DPDK_BINARY_DIR}/lib/librte_pmd_ixgbe.a
  ${DPDK_BINARY_DIR}/lib/librte_pmd_kni.a
  ${DPDK_BINARY_DIR}/lib/librte_pmd_lio.a
  ${DPDK_BINARY_DIR}/lib/librte_pmd_nfp.a
  ${DPDK_BINARY_DIR}/lib/librte_pmd_null.a
  ${DPDK_BINARY_DIR}/lib/librte_pmd_null_crypto.a
  ${DPDK_BINARY_DIR}/lib/librte_pmd_octeontx_ssovf.a
  ${DPDK_BINARY_DIR}/lib/librte_pmd_qede.a
  ${DPDK_BINARY_DIR}/lib/librte_pmd_ring.a
  ${DPDK_BINARY_DIR}/lib/librte_pmd_sfc_efx.a
  ${DPDK_BINARY_DIR}/lib/librte_pmd_skeleton_event.a
  ${DPDK_BINARY_DIR}/lib/librte_pmd_sw_event.a
  ${DPDK_BINARY_DIR}/lib/librte_pmd_tap.a
  ${DPDK_BINARY_DIR}/lib/librte_pmd_thunderx_nicvf.a
  ${DPDK_BINARY_DIR}/lib/librte_pmd_vhost.a
  ${DPDK_BINARY_DIR}/lib/librte_pmd_virtio.a
  ${DPDK_BINARY_DIR}/lib/librte_pmd_vmxnet3_uio.a
  ${DPDK_BINARY_DIR}/lib/librte_port.a
  ${DPDK_BINARY_DIR}/lib/librte_power.a
  ${DPDK_BINARY_DIR}/lib/librte_reorder.a
  ${DPDK_BINARY_DIR}/lib/librte_ring.a
  ${DPDK_BINARY_DIR}/lib/librte_sched.a
  ${DPDK_BINARY_DIR}/lib/librte_table.a
  ${DPDK_BINARY_DIR}/lib/librte_timer.a
  ${DPDK_BINARY_DIR}/lib/librte_vhost.a
)
add_custom_target(dpdk
  WORKING_DIRECTORY ${DPDK_SOURCE_DIR}
  COMMAND mkdir -p ${DPDK_SOURCE_DIR}/${DPDK_TARGET}/include
  COMMAND mkdir -p ${DPDK_SOURCE_DIR}/${DPDK_TARGET}/lib
  COMMAND make config T=${DPDK_TARGET} DESTDIR=${DPDK_BINARY_DIR}
  COMMAND make install T=${DPDK_TARGET} DESTDIR=${DPDK_BINARY_DIR}
)
function(target_add_dpdk TARGET)
  target_link_libraries(${TARGET} ${DPDK_LIBS})
  target_include_directories(${TARGET} PUBLIC ${DPDK_SOURCE_DIR}/${DPDK_TARGET}/include)
  set_target_properties(${TARGET} PROPERTIES
    COMPILE_FLAGS "-march=native -DRTE_MACHINE_CPUFLAG_SSE -DRTE_MACHINE_CPUFLAG_SSE2 -DRTE_MACHINE_CPUFLAG_SSE3 -DRTE_MACHINE_CPUFLAG_SSSE3 -DRTE_MACHINE_CPUFLAG_SSE4_1 -DRTE_MACHINE_CPUFLAG_SSE4_2"
    LINK_FLAGS "-L${DPDK_SOURCE_DIR}/${DPDK_TARGET}/lib -Wl,-lrte_pipeline -Wl,-lrte_table -Wl,-lrte_port -Wl,-lrte_pdump -Wl,-lrte_distributor -Wl,-lrte_ip_frag -Wl,-lrte_gro -Wl,-lrte_meter -Wl,-lrte_sched -Wl,-lrte_lpm -Wl,--whole-archive -Wl,-lrte_acl -Wl,--no-whole-archive -Wl,-lrte_jobstats -Wl,-lrte_metrics -Wl,-lrte_bitratestats -Wl,-lrte_latencystats -Wl,-lrte_power -Wl,-lrte_timer -Wl,-lrte_efd -Wl,-lrte_cfgfile -Wl,--whole-archive -Wl,-lrte_hash -Wl,-lrte_vhost -Wl,-lrte_kvargs -Wl,-lrte_mbuf -Wl,-lrte_net -Wl,-lrte_ethdev -Wl,-lrte_cryptodev -Wl,-lrte_eventdev -Wl,-lrte_mempool -Wl,-lrte_mempool_ring -Wl,-lrte_ring -Wl,-lrte_eal -Wl,-lrte_cmdline -Wl,-lrte_reorder -Wl,-lrte_kni -Wl,-lrte_mempool_stack -Wl,-lrte_pmd_af_packet -Wl,-lrte_pmd_ark -Wl,-lrte_pmd_avp -Wl,-lrte_pmd_bnxt -Wl,-lrte_pmd_bond -Wl,-lrte_pmd_cxgbe -Wl,-lrte_pmd_e1000 -Wl,-lrte_pmd_ena -Wl,-lrte_pmd_enic -Wl,-lrte_pmd_fm10k -Wl,-lrte_pmd_failsafe -Wl,-lrte_pmd_i40e -Wl,-lrte_pmd_ixgbe -Wl,-lrte_pmd_kni -Wl,-lrte_pmd_lio -Wl,-lrte_pmd_nfp -Wl,-lrte_pmd_null -Wl,-lrte_pmd_qede -Wl,-lrte_pmd_ring -Wl,-lrte_pmd_sfc_efx -Wl,-lrte_pmd_tap -Wl,-lrte_pmd_thunderx_nicvf -Wl,-lrte_pmd_virtio -Wl,-lrte_pmd_vhost -Wl,-lrte_pmd_vmxnet3_uio -Wl,-lrte_pmd_null_crypto -Wl,-lrte_pmd_crypto_scheduler -Wl,-lrte_pmd_skeleton_event -Wl,-lrte_pmd_sw_event -Wl,-lrte_pmd_octeontx_ssovf -Wl,--no-whole-archive -Wl,-lrt -Wl,-lm -Wl,-lnuma -Wl,-ldl -libverbs"
  )
  add_dependencies(${TARGET} dpdk)
endfunction(target_add_dpdk)

# mtcp
set(MTCP_SOURCE_DIR ${CMAKE_SOURCE_DIR}/submodules/mtcp)
set(MTCP_LIBS ${MTCP_SOURCE_DIR}/mtcp/lib/libmtcp.a)
add_custom_target(mtcp
  WORKING_DIRECTORY ${MTCP_SOURCE_DIR}
  DEPENDS ${DPDK_LIBS}
  COMMAND ./configure --with-dpdk-lib=${DPDK_SOURCE_DIR}/${DPDK_TARGET} CFLAGS=-I${CMAKE_SOURCE_DIR}
  COMMAND make
)
function(target_add_mtcp TARGET)
  target_add_dpdk(${TARGET})
  target_link_libraries(${TARGET} ${MTCP_LIBS})
  target_include_directories(${TARGET} PUBLIC ${MTCP_SOURCE_DIR}/mtcp/include)
  add_dependencies(${TARGET} mtcp)
endfunction(target_add_mtcp)
