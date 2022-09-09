#!/bin/bash

# Copyright (c) Microsoft Corporation.
# Licensed under the MIT license.

set -e

PACKAGES="librdmacm-dev libmnl-dev build-essential clang libnuma-dev pkg-config python3 python3-pip meson clang-format"

apt-get update
apt-get -y install $PACKAGES

pip3 install pyelftools
