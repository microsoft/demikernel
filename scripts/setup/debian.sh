#!/bin/bash

# Copyright (c) Microsoft Corporation.
# Licensed under the MIT license.

set -e

APT_PACKAGES="cmake-curses-gui cmake-qt-gui build-essential libgmp-dev libnuma-dev libmnl-dev libelf-dev libboost-dev libboost-program-options-dev libboost-coroutine-dev libboost-system-dev libboost-chrono-dev libyaml-cpp-dev clang libpcre3-dev"

repo_root=$(git rev-parse --show-toplevel)

apt-get -y install $APT_PACKAGES

#$SHELL ${repo_root}/submodules/spdk/scripts/pkgdep.sh
