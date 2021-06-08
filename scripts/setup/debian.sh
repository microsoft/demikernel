#!/bin/bash

# Copyright (c) Microsoft Corporation.
# Licensed under the MIT license.

set -e

APT_PACKAGES="cmake-curses-gui cmake-qt-gui build-essential libnuma-dev libmnl-dev libelf-dev libboost-dev libboost-program-options-dev libboost-coroutine-dev libboost-system-dev libboost-chrono-dev libyaml-cpp-dev libpcap-dev python3 python3-pip"

repo_root=$(git rev-parse --show-toplevel)

apt-get update
apt-get -y install $APT_PACKAGES

pip3 install pyelftools ninja meson

$SHELL ${repo_root}/submodules/spdk/scripts/pkgdep.sh
