# Copyright (c) Microsoft Corporation.
# Licensed under the MIT license.

if(NOT DPDK_DOT_CMAKE_INCLUDED)
set(DPDK_DOT_CMAKE_INCLUDED YES)

include(ExternalProject)
include(list)
include(azure)

set(DPDK_TARGET x86_64-native-linuxapp-gcc CACHE STRING "The DPDK Target")

# DPDK
set(DPDK_SOURCE_DIR ${CMAKE_SOURCE_DIR}/submodules/dpdk)

set(DPDK_BUILD_DIR ${CMAKE_BINARY_DIR}/ExternalProject/dpdk-build)

set(DPDK_INSTALL_DIR ${CMAKE_BINARY_DIR}/ExternalProject/dpdk)
set(DPDK_INCLUDE_DIR ${DPDK_INSTALL_DIR}/include)
set(DPDK_LIB_DIR ${DPDK_INSTALL_DIR}/lib)

set(DPDK_PKGCONFIG_PATH ${DPDK_LIB_DIR}/pkgconfig)
set(DPDK_CFLAGS_FILE ${DPDK_INSTALL_DIR}/cflags.txt)
set(DPDK_LDFLAGS_FILE ${DPDK_INSTALL_DIR}/ldflags.txt)

# TODO: Add support for configuring the DPDK build from our CMAKE_BUILD_TYPE
# if(CMAKE_BUILD_TYPE MATCHES "Rel")
#     set(DPDK_EXTRA_CFLAGS "-fPIC -O3")
# else(CMAKE_BUILD_TYPE MATCHES "Rel")
#     set(DPDK_EXTRA_CFLAGS "-fPIC -O0 -g3 -mno-rdrnd -D_FORTIFY_SOURCE -fstack-protector-strong")
# endif(CMAKE_BUILD_TYPE MATCHES "Rel")
# if(CMAKE_VERBOSE_MAKEFILE)
#     set(DPDK_VERBOSE_MAKEFILE "V=1")
# endif(CMAKE_VERBOSE_MAKEFILE)

# See https://mesonbuild.com/Builtin-options.html for meson options
ExternalProject_Add(dpdk
    PREFIX ${DPDK_SOURCE_DIR}
    SOURCE_DIR ${DPDK_SOURCE_DIR}
    BINARY_DIR ${DPDK_SOURCE_DIR}

    CONFIGURE_COMMAND 
    	# Set --libdir to just "lib" (defaults to "lib/x86_64-linux-gnu") since SPDK requires it.
    	COMMAND meson --prefix=${DPDK_INSTALL_DIR} --libdir=lib --debug --buildtype=release ${DPDK_BUILD_DIR}
    	# See spdk:/dpdkbuild/Makefile: SPDK's build doesn't know to include libbsd if it's available
    	# on the system and DPDK decides to use it.
	COMMAND sed -i "s/#define RTE_USE_LIBBSD .*//g" ${DPDK_BUILD_DIR}/rte_build_config.h 

    BUILD_COMMAND ninja -C ${DPDK_BUILD_DIR} 
    INSTALL_COMMAND meson install -C ${DPDK_BUILD_DIR}

    COMMAND sh -c "PKG_CONFIG_PATH=${DPDK_PKGCONFIG_PATH} pkg-config --cflags libdpdk > ${DPDK_CFLAGS_FILE}"
    COMMAND sh -c "PKG_CONFIG_PATH=${DPDK_PKGCONFIG_PATH} pkg-config --libs libdpdk > ${DPDK_LDFLAGS_FILE}" 
    # HAX: See the comment below. We need to make the LDFLAGS file from pkg-config safe to pass directly
    # to the linker, removing any of the "-Wl," quoting that's used to pass through flags that the C 
    # compiler doesn't understand.
    COMMAND sed -i "s/-Wl,//g" ${DPDK_LDFLAGS_FILE}
)

function(target_add_dpdk TARGET)
    target_include_directories(${TARGET} PUBLIC ${DPDK_INCLUDE_DIR})

    # This is pretty hax: We want to pass in some raw flags to the linker via our DPDK_LDFLAGS_FILE, but
    # CMake interprets "@${DPDK_LDFLAGS_FILE}" as the name of a library and prefixes it with "-l". To 
    # get around this, we use "-Wl," to directly pass the file argument to the linker, skipping `c++`.
    target_link_libraries(${TARGET} "-Wl,@${DPDK_LDFLAGS_FILE}")
    target_link_libraries(${TARGET} "-Wl,-rpath=${DPDK_LIB_DIR}")
    target_link_libraries(${TARGET} "-Wl,--disable-new-dtags")
    set_target_properties(${TARGET} PROPERTIES
	COMPILE_FLAGS @${DPDK_CFLAGS_FILE}
    )
    add_dependencies(${TARGET} dpdk)
endfunction(target_add_dpdk)

endif(NOT DPDK_DOT_CMAKE_INCLUDED)
