# Copyright (c) Microsoft Corporation.
# Licensed under the MIT license.

if(NOT FINDCARGO_DOT_CMAKE_INCLUDED)
set(FINDCARGO_DOT_CMAKE_INCLUDED YES)

include(FindPackageHandleStandardArgs)

find_program(CARGO_EXECUTABLE cargo)
find_package_handle_standard_args(cargo DEFAULT_MSG CARGO_EXECUTABLE)
mark_as_advanced(CARGO_EXECUTABLE)

endif(NOT FINDCARGO_DOT_CMAKE_INCLUDED)
