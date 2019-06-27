add_subdirectory(echo)

add_redis(redis-posix dmtr-libos-posix ${CMAKE_SOURCE_DIR}/submodules/redis-posix)
add_redis(redis-rdma dmtr-libos-rdma ${CMAKE_SOURCE_DIR}/submodules/redis-rdma)
add_redis(redis-lwip dmtr-libos-lwip ${CMAKE_SOURCE_DIR}/submodules/redis-lwip)

set(REDIS_SOURCE_DIR ${CMAKE_SOURCE_DIR}/submodules/redis-vanilla)
set(REDIS_BINARY_DIR ${CMAKE_BINARY_DIR}/ExternalProject/redis-vanilla)
ExternalProject_Add(redis-vanilla
    PREFIX ${REDIS_BINARY_DIR}
    SOURCE_DIR ${REDIS_SOURCE_DIR}
    CONFIGURE_COMMAND echo "No CONFIGURE_COMMAND for target `redis-vanilla`"
    BUILD_COMMAND make -C ${REDIS_SOURCE_DIR} PREFIX=${REDIS_BINARY_DIR} V=1
    INSTALL_COMMAND make -C ${REDIS_SOURCE_DIR} install PREFIX=${REDIS_BINARY_DIR} V=1
)
