#!/bin/bash

XPS=(0 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19);
(let TX=0; for CPUS in "${XPS[@]}"; do
    let mask=0
    for CPU in $CPUS; do let mask=$((mask | 1 << $CPU)); done
    printf %X $mask > /sys/class/net/eno5/queues/tx-$TX/xps_cpus
    let TX+=1
done)
