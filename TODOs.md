# Build
- [ ] Building DPDK creates src/ and tmp/ directories under submodules/dpdk instead of in the build
      directory (build/ExternalProject/dpdk). These directories causes `make dpdk` to fail if the
      developer deletes the build directory and starts from a fresh state.
- [ ] The DPDK dependency management doesn't work: The developer has to call `make dpdk` before
      doing any other build (e.g. `make catnip-libos-echo`).
- [ ] Both [SPDK](https://github.com/sujayakar/spdk-rs) and [DPDK](https://github.com/sujayakar/dpdk-rs)
      require calls to `load_pcie_driver` and `load_mlx5_driver`, respectively. If we
      could get Cargo to pass `--no-as-needed` and `-rpath` to the linker, the developer
      would not need to call this function and set the `LD_LIBRARY_PATH` environment
      variable for Rust builds. See [this section](https://github.com/sujayakar/dpdk-rs#shortcomings)
      repo for more details.

# Catnip
- [ ] Add some warnings when it's been too long since the user entered the kernel. I spent a lot of
      time debugging spurious retransmissions that were killing throughput when really the issue was
      that the application had been hogging the CPU for ~100ms.
- [ ] Generalize DPDK memory management. Currently, we only use zero-copy when the application
      allocates data within a pretty small range (default: 1kb-9kb). Instead, we should use DPDK
      external memory and use a standard memory allocator on a large region of virtual memory pinned
      and registered with DPDK.
- [ ] Clean up the ARP code, which predates the Catnip rewrite and just needs a rewrite itself.

- [ ] Pull out the C API into a separate crate that then calls into a LibOS layer
- [ ] Lift up the LibOS layer to be the public Rust interface
- [ ] Create a "module" trait that lets the developer dynamically install DPDK, SPDK, and RDMA at
      runtime. The file table maps file descriptors to a module, which then implements all of the
      LibOS interface.
  - A Rust Catnip application would link against the LibOS crate and create an empty LibOS with
    no modules loaded at initialization time.
  - Then, it'd additionally link against the DPDK crate and create a `DPDKRuntime`, passing that
    to the Catnip UDP and TCP stacks, which are then dynamically loaded into the LibOS.
  - The application could also initialize SPDK, initialize a module implementing the LibOS API
    for storage, and then load that in as well.

- [ ] SPDK support: There's a prototype on the `spdk-build` branch that has the bindings and
  operations working but fakes the LibOS integration.

- [ ] RDMA support

- [ ] Integration testing for the TCP stack
  - [ ] Add a simulation test that sets up two peers and sends some data back and forth.
  - [ ] Assert some invariants about the execution (e.g. all data eventually makes it)
  - [ ] Psuedorandomly introduce faults (packet drops, reordering) and check that our
        invariants still hold.
  - [ ] Check protocol state coverage.

# C++
- [ ] LWIP build is currently broken: `dmtr_sgalloc` and `dmtr_sgafree` need to be implemented in C
      for all of the C++ queue implementations.
- [ ] Fix TCP and UDP echo server applications -- they have some memory unsafety with their
      memory management for the scatter gather arrays.
