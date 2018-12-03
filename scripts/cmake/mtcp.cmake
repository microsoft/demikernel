if(NOT MTCP_DOT_CMAKE_INCLUDED)
set(MTCP_DOT_CMAKE_INCLUDED YES)

include(ExternalProject)
include(dpdk) # we need access to DPDK-related cache variables

# mTCP's fork of DPDK
set(MTCP_DPDK_SOURCE_DIR ${CMAKE_CURRENT_SOURCE_DIR}/submodules/mtcp/dpdk-17.08)
set(MTCP_DPDK_BINARY_DIR ${CMAKE_CURRENT_BINARY_DIR}/ExternalProject/mtcp_dpdk)
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
  set(MTCP_DPDK_MLX5_PMD_LINK_FLAGS -Wl,-lrte_pmd_mlx5)
else(DPDK_USE_MELLANOX_PMD)
  set(MTCP_DPDK_CONFIG_RTE_LIBRTR_MLX5_PMD n)
endif(DPDK_USE_MELLANOX_PMD)
set(MTCP_DPDK_CONFIG_COMMON_BASE ${MTCP_DPDK_SOURCE_DIR}/config/common_base)
configure_file(${MTCP_DPDK_CONFIG_COMMON_BASE}.in ${MTCP_DPDK_CONFIG_COMMON_BASE})

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
    COMPILE_FLAGS "-march=native -DRTE_MACHINE_CPUFLAG_SSE -DRTE_MACHINE_CPUFLAG_SSE2 -DRTE_MACHINE_CPUFLAG_SSE3 -DRTE_MACHINE_CPUFLAG_SSSE3 -DRTE_MACHINE_CPUFLAG_SSE4_1 -DRTE_MACHINE_CPUFLAG_SSE4_2"
    LINK_FLAGS "-L${MTCP_DPDK_LIB_DIR} -Wl,-lrte_pipeline -Wl,-lrte_table -Wl,-lrte_port -Wl,-lrte_pdump -Wl,-lrte_distributor -Wl,-lrte_ip_frag -Wl,-lrte_gro -Wl,-lrte_meter -Wl,-lrte_sched -Wl,-lrte_lpm -Wl,--whole-archive -Wl,-lrte_acl -Wl,--no-whole-archive -Wl,-lrte_jobstats -Wl,-lrte_metrics -Wl,-lrte_bitratestats -Wl,-lrte_latencystats -Wl,-lrte_power -Wl,-lrte_timer -Wl,-lrte_efd -Wl,-lrte_cfgfile -Wl,--whole-archive -Wl,-lrte_hash -Wl,-lrte_vhost -Wl,-lrte_kvargs -Wl,-lrte_mbuf -Wl,-lrte_net -Wl,-lrte_ethdev -Wl,-lrte_cryptodev -Wl,-lrte_eventdev -Wl,-lrte_mempool -Wl,-lrte_mempool_ring -Wl,-lrte_ring -Wl,-lrte_eal -Wl,-lrte_cmdline -Wl,-lrte_reorder -Wl,-lrte_kni -Wl,-lrte_mempool_stack -Wl,-lrte_pmd_af_packet -Wl,-lrte_pmd_ark -Wl,-lrte_pmd_avp -Wl,-lrte_pmd_bnxt -Wl,-lrte_pmd_bond -Wl,-lrte_pmd_cxgbe -Wl,-lrte_pmd_e1000 -Wl,-lrte_pmd_ena -Wl,-lrte_pmd_enic -Wl,-lrte_pmd_fm10k -Wl,-lrte_pmd_failsafe -Wl,-lrte_pmd_i40e -Wl,-lrte_pmd_ixgbe -Wl,-lrte_pmd_kni -Wl,-lrte_pmd_lio ${MTCP_DPDK_MLX5_PMD_LINK_FLAGS} -Wl,-libverbs -Wl,-lrte_pmd_nfp -Wl,-lrte_pmd_null -Wl,-lrte_pmd_qede -Wl,-lrte_pmd_ring -Wl,-lrte_pmd_sfc_efx -Wl,-lrte_pmd_tap -Wl,-lrte_pmd_thunderx_nicvf -Wl,-lrte_pmd_virtio -Wl,-lrte_pmd_vhost -Wl,-lrte_pmd_vmxnet3_uio -Wl,-lrte_pmd_null_crypto -Wl,-lrte_pmd_crypto_scheduler -Wl,-lrte_pmd_skeleton_event -Wl,-lrte_pmd_sw_event -Wl,-lrte_pmd_octeontx_ssovf -Wl,--no-whole-archive -Wl,-lrt -Wl,-lm -Wl,-lnuma -Wl,-ldl"
  )
endfunction(target_add_mtcp)

endif(NOT MTCP_DOT_CMAKE_INCLUDED)
