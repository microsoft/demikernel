if(NOT PLF_DOT_CMAKE_INCLUDED)
set(PLF_DOT_CMAKE_INCLUDED YES)

function(target_add_plf TARGET)
    target_include_directories(${TARGET} PUBLIC ${CMAKE_SOURCE_DIR}/submodules/plf_nanotimer)
endfunction(target_add_plf)

endif(NOT PLF_DOT_CMAKE_INCLUDED)
