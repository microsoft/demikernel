#!/bin/bash

HOMEDIR='/home/fbc'

ARGS="--calibrate sqrt 1000"
TIMEOUT=120

sudo HOME=$HOMEDIR LD_LIBRARY_PATH=$HOME/lib/x86_64-linux-gnu make test-system-rust TIMEOUT=${TIMEOUT} LIBOS=catnip TEST=tcp-echo-multiflow ARGS="${ARGS}"
