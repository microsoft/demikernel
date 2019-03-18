#!/bin/bash

set -e

<<<<<<< HEAD
APT_PACKAGES="cmake-curses-gui cmake-qt-gui build-essential libgmp-dev libnuma-dev libmnl-dev libelf-dev libboost-dev libboost-program-options-dev libboost-coroutine-dev libboost-system-dev libyaml-cpp-dev libboost-chrono-dev"
=======
APT_PACKAGES="cmake-curses-gui cmake-qt-gui build-essential libgmp-dev libnuma-dev libmnl-dev libelf-dev libboost-dev libboost-program-options-dev libboost-coroutine-dev libboost-system-dev libboost-chrono-dev libyaml-cpp-dev"
>>>>>>> c49ea08db2522a727cf53b262daf1eca6fb35576

repo_root=$(git rev-parse --show-toplevel)

apt-get -y install $APT_PACKAGES

#$SHELL ${repo_root}/submodules/spdk/scripts/pkgdep.sh
