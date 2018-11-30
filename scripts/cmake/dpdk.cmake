if(NOT DPDK_DOT_CMAKE_INCLUDED)
set(DPDK_DOT_CMAKE_INCLUDED YES)

include(ExternalProject)
include(list)

option(DPDK_USE_MELLANOX_PMD "include support for the Mellanox adaptor" OFF)
set(DPDK_TARGET x86_64-native-linuxapp-gcc CACHE STRING "The DPDK Target")

# DPDK
set(DPDK_SOURCE_DIR ${CMAKE_CURRENT_SOURCE_DIR}/submodules/dpdk)
set(DPDK_BINARY_DIR ${CMAKE_CURRENT_BINARY_DIR}/ExternalProject/dpdk)
set(DPDK_INSTALL_DIR ${DPDK_BINARY_DIR})
set(DPDK_INCLUDE_DIR ${DPDK_INSTALL_DIR}/include ${DPDK_INSTALL_DIR}/include/dpdk)
set(DPDK_LIB_DIR ${DPDK_INSTALL_DIR}/lib)
set(DPDK_LIBS
    dpdk
    rte_acl
    rte_bbdev
    rte_bitratestats
    rte_bpf
    rte_bus_dpaa
    rte_bus_fslmc
    rte_bus_ifpga
    rte_bus_pci
    rte_bus_vdev
    rte_bus_vmbus
    rte_cfgfile
    rte_cmdline
    rte_common_cpt
    rte_common_dpaax
    rte_common_octeontx
    rte_compressdev
    rte_cryptodev
    rte_distributor
    rte_eal
    rte_efd
    rte_ethdev
    rte_eventdev
    rte_flow_classify
    rte_gro
    rte_gso
    rte_hash
    rte_ip_frag
    rte_jobstats
    rte_kni
    rte_kvargs
    rte_latencystats
    rte_lpm
    rte_mbuf
    rte_member
    rte_mempool
    rte_mempool_bucket
    rte_mempool_dpaa2
    rte_mempool_dpaa
    rte_mempool_octeontx
    rte_mempool_ring
    rte_mempool_stack
    rte_meter
    rte_metrics
    rte_net
    rte_pci
    rte_pdump
    rte_pipeline
    rte_pmd_af_packet
    rte_pmd_ark
    rte_pmd_atlantic
    rte_pmd_avf
    rte_pmd_avp
    rte_pmd_axgbe
    rte_pmd_bbdev_null
    rte_pmd_bnxt
    rte_pmd_bond
    rte_pmd_caam_jr
    rte_pmd_crypto_scheduler
    rte_pmd_cxgbe
    rte_pmd_dpaa2
    rte_pmd_dpaa2_cmdif
    rte_pmd_dpaa2_event
    rte_pmd_dpaa2_qdma
    rte_pmd_dpaa2_sec
    rte_pmd_dpaa
    rte_pmd_dpaa_event
    rte_pmd_dpaa_sec
    rte_pmd_dsw_event
    rte_pmd_e1000
    rte_pmd_ena
    rte_pmd_enetc
    rte_pmd_enic
    rte_pmd_failsafe
    rte_pmd_fm10k
    rte_pmd_i40e
    rte_pmd_ifc
    rte_pmd_ifpga_rawdev
    rte_pmd_ixgbe
    rte_pmd_kni
    rte_pmd_lio
    rte_pmd_netvsc
    rte_pmd_nfp
    rte_pmd_null
    rte_pmd_null_crypto
    rte_pmd_octeontx
    rte_pmd_octeontx_crypto
    rte_pmd_octeontx_ssovf
    rte_pmd_octeontx_zip
    rte_pmd_opdl_event
    rte_pmd_qat
    rte_pmd_qede
    rte_pmd_ring
    rte_pmd_sfc_efx
    rte_pmd_skeleton_event
    rte_pmd_skeleton_rawdev
    rte_pmd_softnic
    rte_pmd_sw_event
    rte_pmd_tap
    rte_pmd_thunderx_nicvf
    rte_pmd_vdev_netvsc
    rte_pmd_vhost
    rte_pmd_virtio
    rte_pmd_virtio_crypto
    rte_pmd_vmxnet3_uio
    rte_port
    rte_power
    rte_rawdev
    rte_reorder
    rte_ring
    rte_sched
    rte_security
    rte_table
    rte_timer
    rte_vhost
)
ExternalProject_Add(dpdk
    PREFIX ${DPDK_BINARY_DIR}
    SOURCE_DIR ${DPDK_SOURCE_DIR}
    CONFIGURE_COMMAND make -C ${DPDK_SOURCE_DIR} config  T=${DPDK_TARGET}
    BUILD_COMMAND make -C ${DPDK_SOURCE_DIR} T=${DPDK_TARGET}
    INSTALL_COMMAND make -C ${DPDK_SOURCE_DIR} install T=${DPDK_TARGET} DESTDIR=${DPDK_INSTALL_DIR}
)
if(DPDK_USE_MELLANOX_PMD)
    set(DPDK_LIBS ${DPDK_LIBS} rte_pmd_mlx5)
    set(DPDK_CONFIG_RTE_LIBRTR_MLX5_PMD y)
else(DPDK_USE_MELLANOX_PMD)
    set(DPDK_CONFIG_RTE_LIBRTR_MLX5_PMD n)
endif(DPDK_USE_MELLANOX_PMD)
set(DPDK_CONFIG_COMMON_BASE ${DPDK_SOURCE_DIR}/config/common_base)
configure_file(${DPDK_CONFIG_COMMON_BASE}.in ${DPDK_CONFIG_COMMON_BASE})
function(target_add_dpdk TARGET)
    list_map_prepend(DPDK_LIB_FLAGS "-Wl,-l" ${DPDK_LIBS})
    string(REPLACE ";" " " DPDK_LIB_FLAGS "${DPDK_LIB_FLAGS}")
    target_include_directories(${TARGET} PUBLIC ${DPDK_INCLUDE_DIR})
    set_target_properties(${TARGET} PROPERTIES
        COMPILE_FLAGS "-march=native -DRTE_MACHINE_CPUFLAG_SSE -DRTE_MACHINE_CPUFLAG_SSE2 -DRTE_MACHINE_CPUFLAG_SSE3 -DRTE_MACHINE_CPUFLAG_SSSE3 -DRTE_MACHINE_CPUFLAG_SSE4_1 -DRTE_MACHINE_CPUFLAG_SSE4_2"
        LINK_FLAGS "-L${DPDK_LIB_DIR} -Wl,--whole-archive ${DPDK_LIB_FLAGS} -Wl,--no-whole-archive -Wl,-lrt -Wl,-lm -Wl,-lnuma -Wl,-ldl"
    )
    add_dependencies(${TARGET} dpdk)
endfunction(target_add_dpdk)

endif(NOT DPDK_DOT_CMAKE_INCLUDED)
