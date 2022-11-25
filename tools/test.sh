#!/bin/bash

# Copyright (c) Microsoft Corporation.
# Licensed under the MIT license.

THIS_DIR=$( cd -- "$( dirname -- "${BASH_SOURCE[0]}" )" &> /dev/null && pwd )

# Parameters.
LIBOS=$1       # LibOS to build.
BRANCH=$2      # Branch to build.
SERVER=$3      # Server host.
CLIENT=$4      # Client host.
TEST=$5        # Test.
SERVER_ARGS=$6 # Test arguments.
CLIENT_ARGS=$7 # Test arguments.

# Constants.
REPOSITORY=demikernel

# Clean up files from a previous run.
rm *.stdout *.stderr

# # Checkout.
python3 $THIS_DIR/jobs/checkout.py --host $SERVER --repository $REPOSITORY --branch $BRANCH > /dev/null &
server_pid=$!
python3 $THIS_DIR/jobs/checkout.py --host $CLIENT --repository $REPOSITORY --branch $BRANCH > /dev/null &
client_pid=$!
START="$(date +%s)"
wait $server_pid $client_pid
DURATION=$[ $(date +%s) - ${START} ]
status=$?
test $status -eq 0 && printf "[$LIBOS] PASSED in %3d s check out\n" $DURATION || printf "[$LIBOS] PASSED in %3d s check out\n" $DURATION

# Compile release build.
if test $status -eq 0;
then
    python3 $THIS_DIR/jobs/compile.py --host $SERVER --repository $REPOSITORY --libos $LIBOS > /dev/null &
    server_pid=$!
    python3 $THIS_DIR/jobs/compile.py --host $CLIENT --repository $REPOSITORY --libos $LIBOS > /dev/null &
    client_pid=$!
    START="$(date +%s)"
    wait $server_pid $client_pid
    DURATION=$[ $(date +%s) - ${START} ]
    status=$?
    test $status -eq 0 && printf "[$LIBOS] PASSED in %3d s release compilation\n" $DURATION || printf "[$LIBOS] PASSED in %3d s check out\n" $DURATION
fi

# Run system tests for release build.
if test $status -eq 0;
then
    python3 $THIS_DIR/jobs/launch.py --host $SERVER --repository $REPOSITORY --libos $LIBOS --test $TEST --args "--server $SERVER_ARGS" > /dev/null &
    server_pid=$!
    python3 $THIS_DIR/jobs/launch.py --host $CLIENT --repository $REPOSITORY --libos $LIBOS --test $TEST --args "--client $CLIENT_ARGS" > /dev/null  &
    client_pid=$!
    START="$(date +%s)"
    wait $server_pid $client_pid
    DURATION=$[ $(date +%s) - ${START} ]
    status=$?
    test $status -eq 0 && printf "[$LIBOS] PASSED in %3d s $TEST\n" $DURATION || printf "[$LIBOS] PASSED in %3d s $TEST\n" $DURATION
fi

# Cleanup.
python3 $THIS_DIR/jobs/cleanup.py --host $SERVER --repository $REPOSITORY > /dev/null &
server_pid=$!
python3 $THIS_DIR/jobs/cleanup.py --host $CLIENT --repository $REPOSITORY > /dev/null &
client_pid=$!
START="$(date +%s)"
wait $server_pid $client_pid
DURATION=$[ $(date +%s) - ${START} ]
status=$?
test $status -eq 0 && printf "[$LIBOS] PASSED in %3d s cleanup\n" $DURATION || printf "[$LIBOS] PASSED in %3d s check out\n" $DURATION
