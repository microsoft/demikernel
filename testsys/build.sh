#!/bin/bash

# Copyright (c) Microsoft Corporation.
# Licensed under the MIT license.

THIS_DIR=$( cd -- "$( dirname -- "${BASH_SOURCE[0]}" )" &> /dev/null && pwd )

# Parameters.
LIBOS=$1    # LibOS to build.
BRANCH=$2   # Branch to build.
SERVER=$3   # Server host.
CLIENT=$4   # Client host.

# Constants.
PROJECT=demikernel
WORKSPACE=demikernel

status=0

# Checkout.
python3 $THIS_DIR/jobs/checkout.py -w $WORKSPACE -p $PROJECT -s $SERVER -c $CLIENT -b $BRANCH

# Compile debug build.
test $? -eq 0 && python3 $THIS_DIR/jobs/compile.py -w $WORKSPACE -p $PROJECT -s $SERVER -c $CLIENT -l $LIBOS -d || status=1

# Compile release build.
test $? -eq 0 && python3 $THIS_DIR/jobs/compile.py -w $WORKSPACE -p $PROJECT -s $SERVER -c $CLIENT -l $LIBOS || status=1

# Cleanup.
python3 $THIS_DIR/jobs/cleanup.py -w $WORKSPACE -p $PROJECT -s $SERVER -c $CLIENT

test $? -eq 0 || status=1

exit $status
