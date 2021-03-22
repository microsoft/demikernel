To build this crate outside of the CMake build, you need to set the `PKG_CONFIG_PATH` and `LD_LIBRARY_PATH` environment variables.

For example,
```
~/src/demikernel/src/rust/catnip_libos $ LD_LIBRARY_PATH=~/src/dpdk/install/lib/x86_64-linux-gnu PKG_CONFIG_PATH=~/src/dpdk/install/lib/x86_64-linux-gnu/pkgconfig cargo test mbuf
```

Similarly, running a binary outside of Cargo will require setting `LD_LIBRARY_PATH` as well.
