# spdk
set(SPDK_SOURCE_DIR ${CMAKE_SOURCE_DIR}/submodules/spdk)
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
