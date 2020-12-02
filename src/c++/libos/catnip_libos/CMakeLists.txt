file(GLOB CATNIP_LIBOS_SOURCES ${CMAKE_CURRENT_SOURCE_DIR}/*.cc)
add_library(dmtr-catnip_libos SHARED ${CATNIP_LIBOS_SOURCES})

# target_add_mimalloc(dmtr-catnip_libos)
target_add_dpdk(dmtr-catnip_libos)

add_dependencies(dmtr-catnip_libos catnip_libos)
target_include_directories(dmtr-catnip_libos PUBLIC ${CMAKE_SOURCE_DIR}/src/rust/catnip_libos/include)

target_link_libraries(dmtr-catnip_libos boost_program_options yaml-cpp Threads::Threads)

# target_link_libraries(dmtr-catnip_libos "-Wl,--whole-archive" dmtr-libos-common "-Wl,--no-whole-archive")

target_link_libraries(dmtr-catnip_libos "-Wl,--whole-archive" dmtr-libos-base "-Wl,--no-whole-archive")

target_link_libraries(dmtr-catnip_libos "-Wl,--whole-archive -lnuma -Wl,--no-whole-archive")

target_link_libraries(dmtr-catnip_libos boost_context boost_coroutine dmtr-latency)

target_link_libraries(dmtr-catnip_libos ${CMAKE_BINARY_DIR}/src/rust/catnip_libos/lib/libcatnip_libos.a)

target_link_libraries(dmtr-catnip_libos dl)
