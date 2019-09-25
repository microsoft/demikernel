# Copyright (c) Microsoft Corporation.
# Licensed under the MIT license.

if(NOT PCAPPLUSPLUS_DOT_CMAKE_INCLUDED)
set(PCAPPLUSPLUS_DOT_CMAKE_INCLUDED YES)

set(PCAPPLUSPLUS_SOURCE_DIR ${CMAKE_SOURCE_DIR}/submodules/PcapPlusPlus)
set(PCAPPLUSPLUS_BINARY_DIR ${CMAKE_BINARY_DIR}/ExternalProject/PcapPlusPlus)
set(PCAPPLUSPLUS_INSTALL_DIR ${CMAKE_BINARY_DIR}/submodules/PcapPlusPlus)
file(MAKE_DIRECTORY ${PCAPPLUSPLUS_INSTALL_DIR})
set(PCAPPLUSPLUS_LIB_DIR ${PCAPPLUSPLUS_INSTALL_DIR}/lib)
set(PCAPPLUSPLUS_INCLUDE_DIR ${PCAPPLUSPLUS_INSTALL_DIR}/include)
set(PCAPPLUSPLUS_LIBS ${PCAPPLUSPLUS_LIB_DIR}/libCommon++.a ${PCAPPLUSPLUS_LIB_DIR}/libPacket++.a ${PCAPPLUSPLUS_LIB_DIR}/libPcap++.a)

add_custom_command(
    OUTPUT ${PCAPPLUSPLUS_LIBS}
    WORKING_DIRECTORY ${PCAPPLUSPLUS_SOURCE_DIR}
    COMMAND ./configure-linux.sh --default --install-dir ${PCAPPLUSPLUS_INSTALL_DIR}
    COMMAND make all
    COMMAND make install)

add_custom_target(PcapPlusPlus
    DEPENDS ${PCAPPLUSPLUS_LIBS})

function(target_add_PcapPlusPlus TARGET)
    target_link_libraries(${TARGET} ${PCAPPLUSPLUS_LIBS})
    target_include_directories(${TARGET} PRIVATE ${PCAPPLUSPLUS_INCLUDE_DIR})
endfunction(target_add_PcapPlusPlus)

endif(NOT PCAPPLUSPLUS_DOT_CMAKE_INCLUDED)

