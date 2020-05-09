#!/bin/sh

# Copyright (c) Microsoft Corporation.
# Licensed under the MIT license.

# to be run every time the machine is rebooted.

echo 1024 | sudo tee /sys/devices/system/node/node*/hugepages/hugepages-2048kB/nr_hugepages
mkdir /mnt/huge || true
mount -t hugetlbfs nodev /mnt/huge
modprobe ib_uverbs
modprobe mlx4_ib
cpupower frequency-set --governor performance
#sysctl -p
#ethtool -K ens1 lro on
#ifconfig ens1 txqueuelen 20000
#systemctl stop irqbalance
export MLX4_SINGLE_THREADED=1
export MLX5_SINGLE_THREADED=1
export MLX5_SHUT_UP_BF=0
export MLX_QP_ALLOC_TYPE="HUGE"
export MLX_CQ_ALLOC_TYPE="HUGE"