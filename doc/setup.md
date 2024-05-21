# Setting Up Development Environment

This document contains instructions on how to setup a development environment
for Demikernel in Linux.

> The instructions in this file assume that you have at your disposal at one
Linux machine. For more more information about required hardware and
specification, check out the `README.md` file.

## Table of Contents

- [Table of Contents](#table-of-contents)
- [1. Clone This Repository](#1-clone-this-repository)
- [2. Install Third-Party Requirements](#2-install-third-party-requirements)
- [3. Install Rust Toolchain](#3-install-rust-toolchain)
- [4. Build DPDK Library (For Catnip and Only Once)](#4-build-dpdk-library-for-catnip-and-only-once)
- [5. Setup Configuration File (Only Once)](#5-setup-configuration-file-only-once)
- [6. Enable Huge Pages (Only for Catnip on Every System Reboot)](#6-enable-huge-pages-only-for-catnip-on-every-system-reboot)

> **Follow these instructions to build Demikernel on a fresh Ubuntu 22.04 system.**

## 1. Clone This Repository

```bash
export WORKDIR=$HOME                                                  # Change this to whatever you want.
cd $WORKDIR                                                           # Switch to working directory.
git clone --recursive https://github.com/microsoft/demikernel.git    # Recursive clone.
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

## 5. Setup Configuration File (Only Once)

- Copy the template from `scripts/config/default.yaml` to
  `$HOME/config.yaml`. If running on Azure, use `scripts/config/azure.yaml`.
- Open the file in `$HOME/config.yaml` for editing and do the following:
  - Change `XX.XX.XX.XX` to match the IPv4 address that in the local host.
  - Change `ff:ff:ff:ff:ff:ff` to match the MAC address in the local host.
  - Change `abcde` to match the name of the interface in the local host.
  - Change the `arp_table` according to your setup. Each line should contain the MAC address of a host matched to the IP address of the same host.
  - If using DPDK, change `WW:WW.W` to match the PCIe address of your NIC.
- Save the file.

## 6. Enable Huge Pages (Only for Catnip on Every System Reboot)

```bash
sudo -E ./scripts/setup/hugepages.sh
```
