#!/bin/bash
set -e

# note: this file only exists to demonstrate how to build dpdk.
# CMake handles all of this automatically now.

repo_root=$(git rev-parse --show-toplevel)
dpdk_root=${repo_root}/submodules/dpdk

# see http://doc.dpdk.org/guides/linux_gsg/build_dpdk.html for details.
dpdk_target=x86_64-native-linuxapp-gcc

pushd ${dpdk_root} >> /dev/null
trap 'popd >> /dev/null' EXIT

# todo: use DESTDIR=... to put results in CMake build directory.
make T=${dpdk_target}