#!/bin/bash

# Copyright (c) Microsoft Corporation.
# Licensed under the MIT license.

set -e

APT_PACKAGES="libevent-dev libevent-pthreads"

apt-get -y install $APT_PACKAGES
