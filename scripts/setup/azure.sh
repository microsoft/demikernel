#!/bin/bash

# Copyright (c) Microsoft Corporation.
# Licensed under the MIT license.

# Load mlx4 driver.
if [ ! -z "`lspci | grep "ConnectX-3"`" ];
then
    modprobe ib_uverbs
    modprobe mlx4_ib
    modprobe mlx4_core

# Load mlx4 driver.
elif [ ! -z "`lspci | grep -E "ConnectX-[4,5]"`" ];
then
    modprobe ib_uverbs
    modprobe mlx5_ib
    modprobe mlx5_core

else
    echo "error: unsupported device"
    exit 1

fi
