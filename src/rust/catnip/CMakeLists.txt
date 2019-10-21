# Copyright (c) Microsoft Corporation.
# Licensed under the MIT license.

cmake_minimum_required(VERSION 3.10)
project(catnip NONE)

find_package(cargo)
include(ExternalProject)

set(CATNIP_TARGET catnip)
set(CATNIP_SUFFIX src/rust/catnip)
set(CATNIP_BINARY_DIR ${CMAKE_BINARY_DIR}/${CATNIP_SUFFIX})
set(CATNIP_SOURCE_DIR ${CMAKE_SOURCE_DIR}/${CATNIP_SUFFIX})
set(CATNIP_INCLUDE_DIR ${CMAKE_SOURCE_DIR}/${CATNIP_SUFFIX}/include)
set(CATNIP_STATICLIB ${CATNIP_BINARY_DIR}/lib/libcatnip.a})
set(CATNIP_DYNLIB ${CATNIP_BINARY_DIR}/lib/libcatnip.so})
set(CATNIP_CARGOFLAGS build -Z unstable-options --target-dir ${CATNIP_BINARY_DIR}/tmp/cargo --out-dir ${CATNIP_BINARY_DIR}/lib)
if(CMAKE_BUILD_TYPE MATCHES "Rel")
    set(CATNIP_CARGOFLAGS ${CATNIP_CARGOFLAGS} --release)
endif(CMAKE_BUILD_TYPE MATCHES "Rel")

ExternalProject_Add(${CATNIP_TARGET}
    PREFIX ${CATNIP_BINARY_DIR}
    SOURCE_DIR ${CATNIP_SOURCE_DIR}
    CONFIGURE_COMMAND true
    BUILD_IN_SOURCE TRUE
    BUILD_ALWAYS TRUE
    BUILD_COMMAND ${CARGO_EXECUTABLE} ${CATNIP_CARGOFLAGS}
    BUILD_BYPRODUCTS ${CATNIP_STATICLIB} ${CATNIP_DYNLIB}
    INSTALL_COMMAND true
)
