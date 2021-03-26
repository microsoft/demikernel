cmake_minimum_required(VERSION 3.10)

set(CATNIP_LIBOS_TARGET catnip_libos)
set(CATNIP_LIBOS_SUFFIX src/rust/catnip_libos)
set(CATNIP_LIBOS_BINARY_DIR ${CMAKE_BINARY_DIR}/${CATNIP_LIBOS_SUFFIX})
set(CATNIP_LIBOS_SOURCE_DIR ${CMAKE_SOURCE_DIR}/${CATNIP_LIBOS_SUFFIX})
set(CATNIP_LIBOS_DYNLIB ${CATNIP_BINARY_DIR}/lib/libcatnip_libos.so})
set(CATNIP_LIBOS_CARGOFLAGS "build -Z unstable-options --target-dir ${CATNIP_LIBOS_BINARY_DIR}/tmp/cargo --out-dir ${CATNIP_LIBOS_BINARY_DIR}/lib")

# TODO: Pass this in from the DPDK CMake script
set(PKG_CONFIG_PATH ${CMAKE_BINARY_DIR}/ExternalProject/dpdk/lib/pkgconfig)

if(CMAKE_BUILD_TYPE MATCHES "Rel")
    set(CATNIP_LIBOS_CARGOFLAGS ${CATNIP_LIBOS_CARGOFLAGS} )
endif(CMAKE_BUILD_TYPE MATCHES "Rel")

ExternalProject_Add(${CATNIP_LIBOS_TARGET}
    PREFIX ${CATNIP_LIBOS_BINARY_DIR}
    SOURCE_DIR ${CATNIP_LIBOS_SOURCE_DIR}
    CONFIGURE_COMMAND true
    BUILD_IN_SOURCE TRUE
    BUILD_ALWAYS TRUE
    BUILD_COMMAND sh -c "PKG_CONFIG_PATH=${PKG_CONFIG_PATH} cargo ${CATNIP_LIBOS_CARGOFLAGS} --release"
    BUILD_BYPRODUCTS ${CATNIP_LIBOS_DYNLIB}
    INSTALL_COMMAND true
)

function(target_add_catnip TARGET)
	add_dependencies(${TARGET} catnip_libos)
	target_link_libraries(${TARGET} ${CATNIP_LIBOS_DYNLIB}) 
endfunction(target_add_catnip)




