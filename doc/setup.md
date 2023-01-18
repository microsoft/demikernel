# Setting Up Development Environment

This document contains instructions on how to setup a development environment
for Demikernel in Linux.

> The instructions in this file assume that you have at your disposal at one
Linux machine. For more more information about required hardware and
specification, check out the `README.md` file.

## Table of Contents

- [1. Clone This Repository](#1-clone-this-repository)
- [2. Install Third-Party Requirements](#2-install-third-party-requirements)
- [3. Install Rust Toolchain](#3-install-rust-toolchain)
- [4. Build DPDK Library (For Catnip and Only Once)](#4-build-dpdk-library-for-catnip-and-only-once)
- [5. Build IO Uring Library (For Catcollar and Only Once)](#5-build-io-uring-library-for-catcollar-and-only-once)
- [6. Setup Configuration File (Only Once)](#6-setup-configuration-file-only-once)
- [7. Enable Huge Pages (Optional for Catnip at Every System Reboot)](#7-enable-huge-pages-optional-for-catnip-at-every-system-reboot)


> **Follow these instructions to build Demikernel on a fresh Ubuntu 22.04 system.**

## 1. Clone This Repository

```bash
export WORKDIR=$HOME                                                  # Change this to whatever you want.
cd $WORKDIR                                                           # Switch to working directory.
git clone --recursive https://github.com/demikernel/demikernel.git    # Recursive clone.
cd $WORKDIR/demikernel                                                # Switch to repository's source tree.
```

## 2. Install Third-Party Requirements

```bash
# Check what is going to be installed.
cat scripts/setup/debian.sh

# Install third party libraries.
sudo -H scripts/setup/debian.sh
```

## 3. Install Rust Toolchain

```bash
# Get Rust toolchain.
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

## 4. Build DPDK Library (For Catnip and Only Once)

```bash
./scripts/setup/dpdk.sh
```

## 5. Build IO Uring Library (For Catcollar and Only Once)

```bash
./scripts/setup/io_uring.sh
```

## 6. Setup Configuration File (Only Once)

- Copy the template from `scripts/config/default.yaml` to `$HOME/config.yaml`.
- Open the file in `$HOME/config.yaml` for editing and do the following:
  - Change `XX.XX.XX.XX` to match the IPv4 address of your server host.
  - Change `YY.YY.YY.YY` to match the IPv4 address of your client host.
  - Change `PPPP` to the port number that you will expose in the server host.
  - Change `ZZ.ZZ.ZZ.ZZ` to match the IPv4 address that in the local host.
  - Change `ff:ff:ff:ff:ff:ff` to match the MAC address in the local host.
  - Change `abcde` to match the name of the interface in the local host.
  - Change the `arp_table` according to your setup.
  - If using DPDK, change `WW:WW.W` to match the PCIe address of your NIC.
- Save the file.

## 7. Enable Huge Pages (Only for Catnip on Every System Reboot)

```bash
sudo -E ./scripts/setup/hugepages.sh
```
