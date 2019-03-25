#!/bin/bash

servers=8

for ((i=0; i<$servers; i++))
do
    echo "Setting up demeter$i.."
    ssh root@demeter$i mount /opt/demeter
done
