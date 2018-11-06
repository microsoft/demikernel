# Zeus libOS code

cmake_minimum_required (VERSION 3.5.1)
project (Zeus C CXX)

set(THREADS_PREFER_PTHREAD_FLAG ON)
find_package(Threads REQUIRED)

set(ZEUS_DIR ${CMAKE_CURRENT_SOURCE_DIR})

# default CFLAGS
set(COMMON_CFLAGS "-g3 -mssse3 -fstack-protector -fPIC -Wall -Wextra -Wno-uninitialized -Wno-unused-parameter -Wl,-zdefs")
set(CMAKE_C_FLAGS "${CMAKE_C_FLAGS} ${COMMON_CFLAGS}")
set(CMAKE_CXX_FLAGS "${CMAKE_CXX_FLAGS}  ${COMMON_CFLAGS} -std=c++11 -D__STDC_LIMIT_MACROS")

# dpdk
set(DPDK_TARGET x86_64-native-linuxapp-gcc CACHE STRING "The DPDK Target")
# note: we have no choice but to use a customized version of dpdk provided
# by mtcp.
set(DPDK_SOURCE_DIR ${ZEUS_DIR}/submodules/mtcp/dpdk-17.08)
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
set(MTCP_SOURCE_DIR ${ZEUS_DIR}/submodules/mtcp)
set(MTCP_LIBS ${MTCP_SOURCE_DIR}/mtcp/lib/libmtcp.a)
add_custom_command(
  OUTPUT ${MTCP_LIBS}
  WORKING_DIRECTORY ${MTCP_SOURCE_DIR}
  DEPENDS ${DPDK_LIBS}
  COMMAND ./configure --with-dpdk-lib=${DPDK_SOURCE_DIR}/${DPDK_TARGET} CFLAGS=-I${ZEUS_DIR}
  COMMAND make
)
function(target_add_mtcp TARGET)
  target_add_dpdk(${TARGET})
  target_link_libraries(${TARGET} ${MTCP_LIBS})
  target_include_directories(${TARGET} PUBLIC ${MTCP_SOURCE_DIR}/mtcp/include)
endfunction(target_add_mtcp)

# spdk
set(SPDK_SOURCE_DIR ${ZEUS_DIR}/submodules/spdk)
set(SPDK_LIBS
  ${SPDK_SOURCE_DIR}/build/lib/libspdk_app_rpc.a
  ${SPDK_SOURCE_DIR}/build/lib/libspdk_bdev.a
  ${SPDK_SOURCE_DIR}/build/lib/libspdk_bdev_aio.a
  ${SPDK_SOURCE_DIR}/build/lib/libspdk_bdev_malloc.a
  ${SPDK_SOURCE_DIR}/build/lib/libspdk_bdev_null.a
  ${SPDK_SOURCE_DIR}/build/lib/libspdk_bdev_nvme.a
  ${SPDK_SOURCE_DIR}/build/lib/libspdk_bdev_rpc.a
  ${SPDK_SOURCE_DIR}/build/lib/libspdk_bdev_virtio.a
  ${SPDK_SOURCE_DIR}/build/lib/libspdk_blob.a
  ${SPDK_SOURCE_DIR}/build/lib/libspdk_blob_bdev.a
  ${SPDK_SOURCE_DIR}/build/lib/libspdk_blobfs.a
  ${SPDK_SOURCE_DIR}/build/lib/libspdk_conf.a
  ${SPDK_SOURCE_DIR}/build/lib/libspdk_copy.a
  ${SPDK_SOURCE_DIR}/build/lib/libspdk_copy_ioat.a
  ${SPDK_SOURCE_DIR}/build/lib/libspdk_cunit.a
  ${SPDK_SOURCE_DIR}/build/lib/libspdk_env_dpdk.a
  ${SPDK_SOURCE_DIR}/build/lib/libspdk_event.a
  ${SPDK_SOURCE_DIR}/build/lib/libspdk_event_bdev.a
  ${SPDK_SOURCE_DIR}/build/lib/libspdk_event_copy.a
  ${SPDK_SOURCE_DIR}/build/lib/libspdk_event_iscsi.a
  ${SPDK_SOURCE_DIR}/build/lib/libspdk_event_nbd.a
  ${SPDK_SOURCE_DIR}/build/lib/libspdk_event_net.a
  ${SPDK_SOURCE_DIR}/build/lib/libspdk_event_nvmf.a
  ${SPDK_SOURCE_DIR}/build/lib/libspdk_event_scsi.a
  ${SPDK_SOURCE_DIR}/build/lib/libspdk_event_vhost.a
  ${SPDK_SOURCE_DIR}/build/lib/libspdk_ioat.a
  ${SPDK_SOURCE_DIR}/build/lib/libspdk_iscsi.a
  ${SPDK_SOURCE_DIR}/build/lib/libspdk_json.a
  ${SPDK_SOURCE_DIR}/build/lib/libspdk_jsonrpc.a
  ${SPDK_SOURCE_DIR}/build/lib/libspdk_log.a
  ${SPDK_SOURCE_DIR}/build/lib/libspdk_log_rpc.a
  ${SPDK_SOURCE_DIR}/build/lib/libspdk_lvol.a
  ${SPDK_SOURCE_DIR}/build/lib/libspdk_nbd.a
  ${SPDK_SOURCE_DIR}/build/lib/libspdk_net.a
  ${SPDK_SOURCE_DIR}/build/lib/libspdk_net_posix.a
  ${SPDK_SOURCE_DIR}/build/lib/libspdk_nvme.a
  ${SPDK_SOURCE_DIR}/build/lib/libspdk_nvmf.a
  ${SPDK_SOURCE_DIR}/build/lib/libspdk_rpc.a
  ${SPDK_SOURCE_DIR}/build/lib/libspdk_rte_vhost.a
  ${SPDK_SOURCE_DIR}/build/lib/libspdk_scsi.a
  ${SPDK_SOURCE_DIR}/build/lib/libspdk_spdk_mock.a
  ${SPDK_SOURCE_DIR}/build/lib/libspdk_trace.a
  ${SPDK_SOURCE_DIR}/build/lib/libspdk_util.a
  ${SPDK_SOURCE_DIR}/build/lib/libspdk_vbdev_error.a
  ${SPDK_SOURCE_DIR}/build/lib/libspdk_vbdev_gpt.a
  ${SPDK_SOURCE_DIR}/build/lib/libspdk_vbdev_lvol.a
  ${SPDK_SOURCE_DIR}/build/lib/libspdk_vbdev_passthru.a
  ${SPDK_SOURCE_DIR}/build/lib/libspdk_vbdev_split.a
  ${SPDK_SOURCE_DIR}/build/lib/libspdk_vhost.a
  ${SPDK_SOURCE_DIR}/build/lib/libspdk_virtio.a
)
add_custom_command(
  OUTPUT ${SPDK_LIBS}
  WORKING_DIRECTORY ${SPDK_SOURCE_DIR}
  COMMAND ./configure
  COMMAND make
)
function(target_add_spdk TARGET)
  target_link_libraries(${TARGET} ${SPDK_LIBS})
  target_include_directories(${TARGET} PUBLIC ${SPDK_SOURCE_DIR}/include)
endfunction(target_add_spdk)

# todo: can these be compiled into a common library?
set(ZEUS_SOURCE_DIR ${ZEUS_DIR}/libos)
file(GLOB ZEUS_COMMON_SOURCES "${ZEUS_SOURCE_DIR}/common/*.cc")
include_directories(
  ${ZEUS_DIR}/include
  ${ZEUS_DIR}
)

# hoard
set(HOARD_SOURCE_DIR ${ZEUS_SOURCE_DIR}/common/mem)
set(HOARD_LIBS ${HOARD_SOURCE_DIR}/libhoard.so)
add_custom_command(
  OUTPUT ${HOARD_LIBS}
  WORKING_DIRECTORY ${HOARD_SOURCE_DIR}
  COMMAND make
)
function(target_add_hoard TARGET)
  target_link_libraries(${TARGET} ${HOARD_LIBS})
  target_include_directories(${TARGET} PUBLIC
    ${HOARD_SOURCE_DIR}/include
    ${HOARD_SOURCE_DIR}/include/hoard
    ${HOARD_SOURCE_DIR}/include/superblocks
    ${HOARD_SOURCE_DIR}/include/util
    ${HOARD_SOURCE_DIR}/include/zeus
    ${HOARD_SOURCE_DIR}/Heap-Layers
  )
endfunction(target_add_hoard)

# POSIX libos target
file(GLOB ZEUS_POSIX_SOURCES "${ZEUS_SOURCE_DIR}/libposix/*.cc")
add_library(zeus_posix SHARED ${ZEUS_COMMON_SOURCES} ${ZEUS_POSIX_SOURCES})

# LWIP libos target
file(GLOB ZEUS_LIBLWIP_SOURCES ${ZEUS_SOURCE_DIR}/liblwip/*.cc)
add_library(zeus_lwip STATIC ${ZEUS_LIBLWIP_SOURCES} ${ZEUS_COMMON_SOURCES})
target_add_hoard(zeus_lwip)
target_include_directories(zeus_lwip PUBLIC ${ZEUS_SOURCE_DIR}/liblwip ${DPDK_INCLUDE_DIR} )
# todo: these options should not be passed directly to
# `target_link_libraries()`. we should use `find_library()` instead.
target_link_libraries(zeus_lwip "-Wl,--whole-archive -lnuma -Wl,--no-whole-archive")
target_link_libraries(zeus_lwip Threads::Threads)
target_add_dpdk(zeus_lwip)

# MTCP libos target
file(GLOB ZEUS_MTCP_SOURCES "${ZEUS_SOURCE_DIR}/libmtcp/*.cc")
add_library(zeus_mtcp STATIC ${ZEUS_COMMON_SOURCES} ${ZEUS_MTCP_SOURCES})
target_add_hoard(zeus_mtcp)
target_add_mtcp(zeus_mtcp)

# RDMA libos target
file(GLOB ZEUS_RDMA_SOURCES "${ZEUS_SOURCE_DIR}/librdma/*.cc")
add_library(zeus_rdma SHARED ${ZEUS_COMMON_SOURCES} ${ZEUS_RDMA_SOURCES})
target_add_hoard(zeus_rdma)
target_link_libraries(zeus_rdma rdmacm ibverbs)

# spdk libos target
file(GLOB ZEUS_SPDK_LIB "${ZEUS_SOURCE_DIR}/libspdk/*.cc")
add_library(zeus_spdk STATIC ${ZEUS_COMMON_SOURCES} ${ZEUS_SPDK_LIB} ${ZEUS_POSIX_SOURCES})
target_add_hoard(zeus_spdk)
target_add_spdk(zeus_spdk)

add_subdirectory(apps/echo)

#############################################
# applications
# Tapir
set(TAPIR_APP_DIR ${ZEUS_DIR}/apps/tapir)
add_custom_target(tapir
                  COMMAND ZEUS_SRC_DIR=${ZEUS_DIR} make
                  WORKING_DIRECTORY ${TAPIR_APP_DIR})
add_dependencies(tapir zeus_posix)

add_custom_target(tapir-clean
                  COMMAND make clean
                  WORKING_DIRECTORY ${TAPIR_APP_DIR})


# Redis
set(REDIS_APP_DIR ${ZEUS_DIR}/apps/redis)
add_custom_target(redis
                  COMMAND ZEUS_SRC_DIR=${ZEUS_DIR} make
                  WORKING_DIRECTORY ${REDIS_APP_DIR})
