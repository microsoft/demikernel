if(NOT LIST_DOT_CMAKE_INCLUDED)
set(LIST_DOT_CMAKE_INCLUDED YES)

function(list_map_prepend VAR_NAME PREFIX)
   set(L "")
   foreach(I ${ARGN})
      list(APPEND L "${PREFIX}${I}")
   endforeach(I)
   set(${VAR_NAME} "${L}" PARENT_SCOPE)
endfunction(list_map_prepend)

endif(NOT LIST_DOT_CMAKE_INCLUDED)
