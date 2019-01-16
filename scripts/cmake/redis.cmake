if(NOT REDIS_DOT_CMAKE_INCLUDED)
set(REDIS_DOT_CMAKE_INCLUDED YES)

include(ExternalProject)

# redis
function(add_redis REDIS_TARGET LIBOS_TARGET REDIS_SOURCE_DIR)
    set(REDIS_BINARY_DIR ${CMAKE_BINARY_DIR}/ExternalProject/${REDIS_TARGET})
    # note: add `V=1` to the invocation of `make` to see what the redis build
    # is actually doing.
    ExternalProject_Add(${REDIS_TARGET}
        PREFIX ${REDIS_BINARY_DIR}
        SOURCE_DIR ${REDIS_SOURCE_DIR}
        CONFIGURE_COMMAND echo "No CONFIGURE_COMMAND for target `${REDIS_TARGET}`"
        BUILD_COMMAND DEMETER_INCLUDE=${CMAKE_SOURCE_DIR}/include DEMETER_SO=$<TARGET_FILE:${LIBOS_TARGET}> make -C ${REDIS_SOURCE_DIR} MALLOC=libc PREFIX=${REDIS_BINARY_DIR}
        INSTALL_COMMAND DEMETER_INCLUDE=${CMAKE_SOURCE_DIR}/include DEMETER_SO=$<TARGET_FILE:${LIBOS_TARGET}> make -C ${REDIS_SOURCE_DIR} install MALLOC=libc PREFIX=${REDIS_BINARY_DIR}
    )
    add_dependencies(${REDIS_TARGET} ${LIBOS_TARGET})
endfunction(add_redis)

endif(NOT REDIS_DOT_CMAKE_INCLUDED)
