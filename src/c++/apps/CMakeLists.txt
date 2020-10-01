# Copyright (c) Microsoft Corporation.
# Licensed under the MIT license.

add_subdirectory(echo)

add_redis(redis-posix dmtr-libos-posix ${CMAKE_SOURCE_DIR}/submodules/redis-posix)
add_redis(redis-rdma dmtr-libos-rdma ${CMAKE_SOURCE_DIR}/submodules/redis-rdma)
add_redis(redis-lwip dmtr-libos-lwip ${CMAKE_SOURCE_DIR}/submodules/redis-lwip)

add_redis(redis-dpdk-catnip dmtr-catnip_libos ${CMAKE_SOURCE_DIR}/submodules/redis-dpdk-catnip)

set(REDIS_SOURCE_DIR ${CMAKE_SOURCE_DIR}/submodules/redis-vanilla)
set(REDIS_BINARY_DIR ${CMAKE_BINARY_DIR}/ExternalProject/redis-vanilla)
ExternalProject_Add(redis-vanilla
    PREFIX ${REDIS_BINARY_DIR}
    SOURCE_DIR ${REDIS_SOURCE_DIR}
    CONFIGURE_COMMAND echo "No CONFIGURE_COMMAND for target `redis-vanilla`"
    BUILD_COMMAND make -C ${REDIS_SOURCE_DIR} PREFIX=${REDIS_BINARY_DIR} V=1
    INSTALL_COMMAND make -C ${REDIS_SOURCE_DIR} install PREFIX=${REDIS_BINARY_DIR} V=1
)

add_tapir(tapir-posix dmtr-libos-posix ${CMAKE_SOURCE_DIR}/submodules/Hoard/src/libhoard.so ${CMAKE_SOURCE_DIR}/submodules/tapir-posix)
add_tapir(tapir-rdma dmtr-libos-rdma ${CMAKE_SOURCE_DIR}/submodules/HoardRdma/src/libhoard.so ${CMAKE_SOURCE_DIR}/submodules/tapir-rdma)
add_tapir(tapir-lwip dmtr-libos-lwip ${CMAKE_SOURCE_DIR}/submodules/Hoard/src/libhoard.so ${CMAKE_SOURCE_DIR}/submodules/tapir-lwip)

add_tapir(tapir-dpdk-catnip dmtr-catnip_libos ${CMAKE_SOURCE_DIR}/submodules/Hoard/src/libhoard.so ${CMAKE_SOURCE_DIR}/submodules/tapir-dpdk-catnip)
