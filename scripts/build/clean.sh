#!/bin/bash

# Copyright (c) Microsoft Corporation.
# Licensed under the MIT license.

repo_root=$(git rev-parse --show-toplevel)

pushd $repo_root >> /dev/null
trap 'popd >> /dev/null' EXIT

git clean -fdx && git submodule foreach 'git clean -fdx'
