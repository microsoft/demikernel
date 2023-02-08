
# Building Demikernel

This document contains instructions on how to build Demikernel on Linux.

> The instructions in this file assume that you have at your disposal at one
Linux machine with Demikernel's development environment set up. For more
information on how to set up Demikernel's development environment, check out
instructions in the `README.md` file.

## Table of Contents

- [Building Demikernel with Default Parameters](#building-demikernel-with-default-parameters)
- [Installing Artifacts (Optional)](#installing-artifacts-optional)
- [Building API Documentation (Optional)](#building-api-documentation-optional)
- [Custom Build Parameters for Catnip LibOS (Optional)](#custom-build-parameters-for-catnip-libos-optional)
  - [Override Default Path for DPDK Libraries](#override-default-path-for-dpdk-libraries)
  - [Override Path to DPDK Package Config File](#override-path-to-dpdk-package-config-file)
- [Custom Build Parameters for Catcollar LibOS (Optional)](#custom-build-parameters-for-catcollar-libos-optional)
  - [Override Default Path for I/O Uring Libraries](#override-default-path-for-io-uring-libraries)
  - [Override Path to I/O Uring Package Config File](#override-path-to-io-uring-package-config-file)

## Building Demikernel with Default Parameters

```bash
# Builds Demikernel with default LibOS.
# This defaults to LIBOS=catnap.
make

# Build Demikernel with I/O Uring LibOS.
make LIBOS=catcollar

# Build Demikernel with TCP Socket Loopback LibOS.
make LIBOS=catloop

# Build Demikernel with Shared Memory LibOS.
make LIBOS=catmem

# Build Demikernel with Linux Sockets LibOS.
make LIBOS=catnap

# Build Demikernel with DPDK LibOS.
make LIBOS=catnip

# Build Demikernel with Raw Sockets LibOS
make LIBOS=catpowder
```

## Installing Artifacts (Optional)

```bash
# Copies build artifacts to your $HOME directory.
make install

# Copies build artifacts to a specific location.
make install INSTALL_PREFIX=/path/to/location
```

## Building API Documentation (Optional)

```bash
cargo doc --no-deps    # Build API Documentation
cargo doc --open       # Open API Documentation
```

## Custom Build Parameters for Catnip LibOS (Optional)

The following instructions enable you to tweak the building process for Catnip
LibOS.

### Override Default Path for DPDK Libraries

Override this parameter if your `libdpdk` installation is not located in your
`$HOME` directory.

```bash
# Build Catnip LibOS with a custom location for DPDK libraries.
make LIBOS=catnip LD_LIBRARY_PATH=/path/to/dpdk/libs
```

### Override Path to DPDK Package Config File

Override this parameter if your `libdpdk` installation is not located in your
`$HOME` directory.

```bash
# Build Catnip LibOS with a custom location for DPDK package config files.
make PKG_CONFIG_PATH=/path/to/dpdk/pkgconfig
```

## Custom Build Parameters for Catcollar LibOS (Optional)

The following instructions enable you to tweak the building process for Catcollar LibOS.

### Override Default Path for I/O Uring Libraries

Override this parameter if your `liburing` installation is not located in your
`$HOME` directory.

```bash
# Build Catcollar LibOS with a custom location for DPDK libraries.
make LIBOS=catcollar LD_LIBRARY_PATH=/path/to/dpdk/libs
```

### Override Path to I/O Uring Package Config File

Override this parameter if your `liburing` installation is not located in your
`$HOME` directory.

```bash
# Build Catcollar LibOS with a custom location for DPDK package config files.
make PKG_CONFIG_PATH=/path/to/dpdk/pkgconfig
```
