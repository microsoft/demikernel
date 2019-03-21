add_subdirectory(echo)

add_redis(redis-posix dmtr-libos-posix ${CMAKE_SOURCE_DIR}/submodules/redis-posix)
add_redis(redis-rdma dmtr-libos-rdma ${CMAKE_SOURCE_DIR}/submodules/redis-rdma)

# Tapir
set(TAPIR_DIR ${CMAKE_CURRENT_SOURCE_DIR}/tapir)
add_custom_target(posix-tapir
  COMMAND DMTR_CFLAGS=-I${CMAKE_SOURCE_DIR}/include DMTR_LDFLAGS='-L${PROJECT_BINARY_DIR}/libos/posix -ldmtr-libos-posix -Wl,-rpath,${PROJECT_BINARY_DIR}/libos/posix -L${CMAKE_SOURCE_DIR}/submodules/Hoard/src -lhoard -Wl,-rpath,${CMAKE_SOURCE_DIR}/submodules/Hoard/src' make
  WORKING_DIRECTORY ${TAPIR_DIR}
)
add_dependencies(posix-tapir dmtr-libos-posix)

add_custom_target(rdma-tapir
  COMMAND DMTR_CFLAGS=-I${CMAKE_SOURCE_DIR}/include DMTR_LDFLAGS='-lrdmacm -libverbs -L${PROJECT_BINARY_DIR}/libos/rdma -ldmtr-libos-rdma -Wl,-rpath,${PROJECT_BINARY_DIR}/libos/rdma -L${CMAKE_SOURCE_DIR}/submodules/HoardRdma/src -lhoard -Wl,-rpath,${CMAKE_SOURCE_DIR}/submodules/HoardRdma/src' make
    WORKING_DIRECTORY ${TAPIR_DIR}
)
add_dependencies(rdma-tapir dmtr-libos-rdma)


#
# todo: is there a way to get this to run on `make clean`?
add_custom_target(tapir-clean
    COMMAND make clean
    WORKING_DIRECTORY ${TAPIR_DIR}
)

