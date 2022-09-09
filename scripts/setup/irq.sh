#!/bin/bash

# Copyright (c) Microsoft Corporation.
# Licensed under the MIT license.

for d in /proc/irq/*/ ;
do
	echo 1 > $d/smp_affinity 
done

echo 1 > /proc/irq/default_smp_affinity
