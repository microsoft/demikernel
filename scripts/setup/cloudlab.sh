#!/bin/bash

servers=8

for ((i=0; i<$servers; i++))
do
    echo "Setting up demeter$i.."
    ssh root@demeter$i mount /opt/demeter
    ssh root@demeter$i sysctl -w vm.nr_hugepages=512
done
