#!/bin/bash

set -e

repo_root=$(git rev-parse --show-toplevel)
spdk_root=${repo_root}/submodules/spdk

pushd ${spdk_root} >> /dev/null
trap 'popd >> /dev/null' EXIT

./configure
make
./test/unit/unittest.sh