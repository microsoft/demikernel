#!/bin/bash

set -e

repo_root=$(git rev-parse --show-toplevel)
mtcp_root=${repo_root}/submodules/mtcp

export EXTRA_CFLAGS="-fPIC -I${repo_root}"

pushd ${mtcp_root} >> /dev/null
trap 'popd >> /dev/null' EXIT

## environment vairables
. setup_dpdk_env.sh

## setup dpdk first
cd ${mtcp_root}/dpdk-17.08
make config T=${RTE_TARGET} DESTDIR=$RTE_TARGET
make install T=${RTE_TARGET}

dpdk_root=${RTE_SDK}/${RTE_TARGET}

## compile mtcp
cd ${mtcp_root}
./configure --with-dpdk-lib=${dpdk_root} CFLAGS="${EXTRA_CFLAGS}"
make

