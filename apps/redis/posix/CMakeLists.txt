set(REDIS_BINARY_DIR ${CMAKE_BINARY_DIR}/ExternalProject/redis)
set(REDIS_SOURCE_DIR ${CMAKE_SOURCE_DIR}/submodules/redis)
# note: add `V=1` to the invocation of `make` to see what the redis build
# is actually doing.
ExternalProject_Add(redis
    PREFIX ${REDIS_BINARY_DIR}
    SOURCE_DIR ${REDIS_SOURCE_DIR}
    CONFIGURE_COMMAND echo "No CONFIGURE_COMMAND for target `redis`"
    BUILD_COMMAND DEMETER_INCLUDE=${CMAKE_SOURCE_DIR}/include DEMETER_SO=$<TARGET_FILE:zeus_posix> make -C ${REDIS_SOURCE_DIR} MALLOC=libc PREFIX=${REDIS_BINARY_DIR}
    INSTALL_COMMAND DEMETER_INCLUDE=${CMAKE_SOURCE_DIR}/include DEMETER_SO=$<TARGET_FILE:zeus_posix> make -C ${REDIS_SOURCE_DIR} install MALLOC=libc PREFIX=${REDIS_BINARY_DIR}
)
add_dependencies(redis zeus_posix)
