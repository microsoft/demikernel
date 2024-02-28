#!/bin/bash

## Perf
USE_PERF=0

LOCAL='192.168.100.1:12345'
HOMEDIR='/home/fbc'

SERVER_CPUS=$1
SERVER_ARGS=$2

SERVER_LCORES=`echo ${SERVER_CPUS} | sed 's/,/:/g'`

TIMEOUT=20
ARGS="--server ${LOCAL} ${SERVER_LCORES} ${SERVER_ARGS}"

if [ ${USE_PERF} -eq 1 ]; then
    echo 0 | sudo tee /proc/sys/kernel/nmi_watchdog
    echo -1 | sudo tee /proc/sys/kernel/perf_event_paranoid

    SERVER_LCORES_FOR_PERF=`echo ${SERVER_CPUS} | cut -d, -f2-`

    taskset -c ${SERVER_CPUS} sudo HOME=${HOMEDIR} LD_LIBRARY_PATH=$HOME/lib/x86_64-linux-gnu perf stat -C ${SERVER_LCORES_FOR_PERF} -A -d -d -d -x'|' -o output.perf make test-system-rust LIBOS=catnip TEST=tcp-echo-multiflow ARGS="${ARGS}" TIMEOUT=$TIMEOUT

    echo 1 | sudo tee /proc/sys/kernel/nmi_watchdog
else
    taskset -c ${SERVER_CPUS} sudo HOME=${HOMEDIR} LD_LIBRARY_PATH=$HOME/lib/x86_64-linux-gnu make test-system-rust LIBOS=catnip TEST=tcp-echo-multiflow ARGS="${ARGS}" TIMEOUT=$TIMEOUT
fi

## Example to run:
# ./run_server.sh "1,3,5,7,9,11,13,15,17" "8 sqrt"