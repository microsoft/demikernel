#!/bin/bash

servers=7

for ((i=0; i<$servers; i++))
do
    echo "Setting up demeter$i.."
    ssh root@demeter$i ip address delete 192.168.2.1/32 dev ens1f1
    ssh root@demeter$i ip address delete 192.168.2.$i/32 dev ens1f1
    ssh root@demeter$i ip address add 192.168.2.$i/24 dev ens1f1
    ssh root@demeter$i mount /opt/demeter
done
