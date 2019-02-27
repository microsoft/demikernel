#!/bin/bash

set -e

add-apt-repository ppa:canonical-server/dpdk-azure -y
apt-get update
APT_PACKAGES="cmake-curses-gui cmake-qt-gui build-essential libgmp-dev libnuma-dev libmnl-dev librdmacm1 librdmacm-dev libelf-dev libboost-dev libyaml-cpp-dev"

repo_root=$(git rev-parse --show-toplevel)

apt-get -y install $APT_PACKAGES

#$SHELL ${repo_root}/submodules/spdk/scripts/pkgdep.sh
