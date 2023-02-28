#!/bin/bash

# Copyright (c) Microsoft Corporation.
# Licensed under the MIT license.

set -e

PACKAGES="librdmacm-dev libmnl-dev build-essential clang libnuma-dev pkg-config python3 python3-pip meson clang-format"

apt-get update
apt-get -y install $PACKAGES

# This dependency is only needed on machines that drive the
# Azure Tables script for continuous integration.
pip3 install azure-data-tables
