add_subdirectory(echo)

add_redis(redis-posix zeus_posix ${CMAKE_SOURCE_DIR}/submodules/redis-posix)
add_redis(redis-rdma zeus_rdma ${CMAKE_SOURCE_DIR}/submodules/redis-rdma)

# Tapir
set(TAPIR_DIR ${CMAKE_CURRENT_SOURCE_DIR}/tapir)
add_custom_target(tapir
    COMMAND ZEUS_SRC_DIR=${CMAKE_SOURCE_DIR} make
    WORKING_DIRECTORY ${TAPIR_DIR}
)
add_dependencies(tapir zeus_posix)
#
# todo: is there a way to get this to run on `make clean`?
add_custom_target(tapir-clean
    COMMAND make clean
    WORKING_DIRECTORY ${TAPIR_DIR}
)

