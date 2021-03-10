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
set(DPDK_BINARY_DIR ${CMAKE_BINARY_DIR}/ExternalProject/dpdk)
set(DPDK_INSTALL_DIR ${DPDK_BINARY_DIR}/install)
set(DPDK_INCLUDE_DIR ${DPDK_INSTALL_DIR}/include)
set(DPDK_LIB_DIR ${DPDK_INSTALL_DIR}/lib/x86_64-linux-gnu)

# TODO: Add support for configuring the DPDK build from our CMAKE_BUILD_TYPE
# if(CMAKE_BUILD_TYPE MATCHES "Rel")
#     set(DPDK_EXTRA_CFLAGS "-fPIC -O3")
# else(CMAKE_BUILD_TYPE MATCHES "Rel")
#     set(DPDK_EXTRA_CFLAGS "-fPIC -O0 -g3 -mno-rdrnd -D_FORTIFY_SOURCE -fstack-protector-strong")
# endif(CMAKE_BUILD_TYPE MATCHES "Rel")
# if(CMAKE_VERBOSE_MAKEFILE)
#     set(DPDK_VERBOSE_MAKEFILE "V=1")
# endif(CMAKE_VERBOSE_MAKEFILE)

# warning: the same build flags have to be passed to both the build command
# and the install command (or `EXTRA_CFLAGS` could be wiped out during
# install).

set(DPDK_PKGCONFIG_PATH ${DPDK_LIB_DIR}/pkgconfig)
set(DPDK_CFLAGS_FILE ${DPDK_INSTALL_DIR}/cflags.txt)
set(DPDK_LDFLAGS_FILE ${DPDK_INSTALL_DIR}/ldflags.txt)

# See https://mesonbuild.com/Builtin-options.html for meson options
ExternalProject_Add(dpdk
    PREFIX ${DPDK_SOURCE_DIR}
    SOURCE_DIR ${DPDK_SOURCE_DIR}
    CONFIGURE_COMMAND meson --buildtype=release --debug --prefix=${DPDK_INSTALL_DIR} ${DPDK_BINARY_DIR} ${DPDK_SOURCE_DIR}
    BUILD_COMMAND ninja -C ${DPDK_BINARY_DIR}
    INSTALL_COMMAND ninja -C ${DPDK_BINARY_DIR} install
    COMMAND sh -c "PKG_CONFIG_PATH=${DPDK_PKGCONFIG_PATH} pkg-config --cflags libdpdk > ${DPDK_CFLAGS_FILE}"
    COMMAND sh -c "PKG_CONFIG_PATH=${DPDK_PKGCONFIG_PATH} pkg-config --libs libdpdk > ${DPDK_LDFLAGS_FILE}" 
    # HAX: See the comment below. We need to make the LDFLAGS file from pkg-config safe to pass directly
    # to the linker, removing any of the "-Wl," quoting that's used to pass through flags that the C 
    # compiler doesn't understand.
    COMMAND sed -i"" "s/-Wl,//g" ${DPDK_LDFLAGS_FILE}
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
