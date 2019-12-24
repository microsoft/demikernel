# Copyright (c) Microsoft Corporation.
# Licensed under the MIT license.

add_subdirectory(echo)
add_subdirectory(kv)
add_subdirectory(nc)
add_subdirectory(clients)
add_subdirectory(http)

#add_redis(redis-posix dmtr-libos-posix psp-libos-common ${CMAKE_SOURCE_DIR}/submodules/redis-posix)
#add_redis(redis-rdma dmtr-libos-rdma psp-libos-common ${CMAKE_SOURCE_DIR}/submodules/redis-rdma)
#add_redis(redis-lwip dmtr-libos-lwip psp-libos-common ${CMAKE_SOURCE_DIR}/submodules/redis-lwip)

#set(REDIS_SOURCE_DIR ${CMAKE_SOURCE_DIR}/submodules/redis-vanilla)
#set(REDIS_BINARY_DIR ${CMAKE_BINARY_DIR}/ExternalProject/redis-vanilla)
#ExternalProject_Add(redis-vanilla
#    PREFIX ${REDIS_BINARY_DIR}
#    SOURCE_DIR ${REDIS_SOURCE_DIR}
#    CONFIGURE_COMMAND echo "No CONFIGURE_COMMAND for target `redis-vanilla`"
#    BUILD_COMMAND make -C ${REDIS_SOURCE_DIR} PREFIX=${REDIS_BINARY_DIR} V=1
#    INSTALL_COMMAND make -C ${REDIS_SOURCE_DIR} install PREFIX=${REDIS_BINARY_DIR} V=1
#)

#add_tapir(tapir-posix dmtr-libos-posix psp-libos-common ${CMAKE_SOURCE_DIR}/submodules/Hoard/src/libhoard.so ${CMAKE_SOURCE_DIR}/submodules/tapir-posix)
#add_tapir(tapir-rdma dmtr-libos-rdma psp-libos-common ${CMAKE_SOURCE_DIR}/submodules/HoardRdma/src/libhoard.so ${CMAKE_SOURCE_DIR}/submodules/tapir-rdma)
#add_tapir(tapir-lwip dmtr-libos-lwip psp-libos-common ${CMAKE_SOURCE_DIR}/submodules/Hoard/src/libhoard.so ${CMAKE_SOURCE_DIR}/submodules/tapir-lwip)
