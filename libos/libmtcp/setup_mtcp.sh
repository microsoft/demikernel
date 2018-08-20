#! /bin/bash
#################################################################################
#     File Name           :     setup_mtcp.sh
#     Created By          :     jingliu
#     Creation Date       :     [2018-08-20 13:09]
#     Last Modified       :     [2018-08-20 15:58]
#     Description         :     setup mtcp for libos
#################################################################################

set -e

CUR_DIR=`pwd`
MTCP_REPO="https://github.com/jingliu9/mtcp.git"


git clone ${MTCP_REPO}

export EXTRA_CFLAGS=-fPIC

## environment vairables
cd ${CUR_DIR}/mtcp/
source setup_dpdk_env.sh

echo $RTE_SDK
echo $RTE_TARGET

## setup dpdk first
cd ${CUR_DIR}/mtcp/dpdk-17.08
make config T=${RTE_TARGET} DESTDIR=$RTE_TARGET
make install T=${RTE_TARGET}

cd ${CUR_DIR}/mtcp/dpdk/

ln -s $RTE_SDK/$RTE_TARGET/lib lib
ln -s $RTE_SDK/$RTE_TARGET/include include

## compile mtcp
cd ${CUR_DIR}/mtcp/
./configure --with-dpdk-lib=${CUR_DIR}/mtcp/dpdk/
make -j 32


