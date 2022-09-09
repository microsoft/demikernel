#!/bin/bash

# Copyright (c) Microsoft Corporation.
# Licensed under the MIT license.

export LIBURING_VERSION=ecf66cc2a49c69e9c9d224ef54b95082844a7b7a

# Fail on error.
set -e

# Switch to working directory.
pushd $PWD
mkdir -p $HOME/tmp
cd $HOME/tmp

# Get sources
git clone https://github.com/axboe/liburing.git
cd liburing
git checkout $LIBURING_VERSION

# Build and install.
./configure --prefix=$HOME
make
make install

# Cleanup.
popd
rm -rf $HOME/tmp/liburing