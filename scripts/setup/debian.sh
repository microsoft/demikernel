#!/bin/bash

set -e

APT_PACKAGES="libgmp-dev libnuma-dev"

repo_root=$(git rev-parse --show-toplevel)

apt-get -y install $APT_PACKAGES

$SHELL ${repo_root}/submodules/spdk/scripts/pkgdep.sh