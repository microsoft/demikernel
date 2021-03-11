cmake_minimum_required(VERSION 3.10)

set(CATNIP_LIBOS_TARGET catnip_libos)
set(CATNIP_LIBOS_SUFFIX src/rust/catnip_libos)
set(CATNIP_LIBOS_BINARY_DIR ${CMAKE_BINARY_DIR}/${CATNIP_LIBOS_SUFFIX})
set(CATNIP_LIBOS_SOURCE_DIR ${CMAKE_SOURCE_DIR}/${CATNIP_LIBOS_SUFFIX})
set(CATNIP_LIBOS_DYNLIB ${CATNIP_BINARY_DIR}/lib/libcatnip_libos.so})
set(CATNIP_LIBOS_CARGOFLAGS "build -Z unstable-options --target-dir ${CATNIP_LIBOS_BINARY_DIR}/tmp/cargo --out-dir ${CATNIP_LIBOS_BINARY_DIR}/lib")

# TODO: Pass this in from the DPDK CMake script
set(PKG_CONFIG_PATH ${CMAKE_BINARY_DIR}/ExternalProject/dpdk/install/lib/x86_64-linux-gnu/pkgconfig)

if(CMAKE_BUILD_TYPE MATCHES "Rel")
    set(CATNIP_LIBOS_CARGOFLAGS ${CATNIP_LIBOS_CARGOFLAGS} --release)
endif(CMAKE_BUILD_TYPE MATCHES "Rel")

# add_custom_command(OUTPUT ${CATNIP_LIBOS_DYNLIB}
# 	COMMAND sh -c "PKG_CONFIG_PATH=${PKG_CONFIG_PATH} ${CARGO_EXECUTABLE} ${CATNIP_LIBOS_CARGOFLAGS}"
# 	WORKING_DIRECTORY ${CATNIP_LIBOS_SOURCE_DIR}
# 	COMMENT "Calling cargo"
# 	VERBATIM
# )
# add_custom_target(catnip_libos DEPENDS ${CATNIP_LIBOS_DYNLIB})
# target_link_libraries(catnip_libos "blah")

add_library(catnip_libos2 SHARED IMPORTED) # target_link_libraries(catnip_libos3 ${CATNIP_LIBOS_DYNLIB})
add_dependencies(catnip_libos2 build_catnip_libos)
set_target_properties(catnip_libos2 PROPERTIES IMPORTED_LOCATION ${CMAKE_BINARY_DIR}/src/rust/catnip_libois/lib/libcatnip_libos.so)

ExternalProject_Add(${CATNIP_LIBOS_TARGET}
    PREFIX ${CATNIP_LIBOS_BINARY_DIR}
    SOURCE_DIR ${CATNIP_LIBOS_SOURCE_DIR}
    CONFIGURE_COMMAND true
    BUILD_IN_SOURCE TRUE
    BUILD_ALWAYS TRUE
    BUILD_COMMAND sh -c "PKG_CONFIG_PATH=${PKG_CONFIG_PATH} ${CARGO_EXECUTABLE} ${CATNIP_LIBOS_CARGOFLAGS}"
    BUILD_BYPRODUCTS ${CATNIP_LIBOS_DYNLIB}
    INSTALL_COMMAND true
)

function(target_add_catnip TARGET)
	add_dependencies(${TARGET} catnip_libos)
	target_link_libraries(${TARGET} ${CATNIP_LIBOS_DYNLIB}) 
endfunction(target_add_catnip)

# target_link_libraries(catnip_libos ${CMAKE_BINARY_DIR}/src/rust/catnip_libos/lib/libcatnip_libos.so)



