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


if [ -z "RTE_SDK" ]
then
    echo "RTE_SDK not set (source setup_dpdk_env.sh in mtcp directory)"
    exit
fi

if [ -z "RTE_TARGET" ]
then
    echo "RTE_TARGET not set"
    exit
fi

# make sure pkgdep.sh have the right to install packages
sudo ./scripts/pkgdep.sh

echo "dependency installed"

configure --with-dpdk=${RTE_SDK}/${RTS_TARGET}

make

cd ${LIBOS_SPDK_DIR}
