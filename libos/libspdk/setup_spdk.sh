#! /bin/bash
#################################################################################
#     File Name           :     setup_spdk.sh
#     Description         :     setup spdk while initilizing this repo
#################################################################################

GIT_URL="https://github.com/jingliu9/spdk.git"
GIT_BRANCH="libos"
# debug branch
# GIT_BRANCH="learning"

LIBOS_SPDK_DIR=`pwd`

git clone ${GIT_URL}

cd spdk
git checkout ${GIT_BRANCH}
# install dependency, compile
git submodule update --init
# make sure pkgdep.sh have the right to install packages
sudo ./scripts/pkgdep.sh

./configure

make

cd ${LIBOS_SPDK_DIR}
