#!/bin/bash

source "$(dirname "${BASH_SOURCE[0]}")/env.bash"

BUFFER_SIZE=1024
NCLIENTS=8

if [ -z "$1" ]; then
    echo "Please input the server address."
    echo "Usage: run-client.bash {address}"
    exit 1
fi

SERVER_HOST=$1

echo " *******        Client Mode         ********************* "
echo " LIBOS:               $LIBOS"
echo " CONFIG:              $CONFIG_PATH"
echo " Host:                $SERVER_HOST"
echo " Client:              $CLIENT_HOST"
echo " Concurrent:          $NCLIENTS"
echo " Buffersize:          $BUFFER_SIZE"
echo "**********************************************************"
echo " "

# 1. tcp-echo client
echo "Starting TCP echo client..."
bin/examples/rust/tcp-echo.elf --peer client --address "$SERVER_HOST:$TCP_SERVER_PORT" --nclients $NCLIENTS --bufsize $BUFFER_SIZE --run-mode concurrent

# 2. udp-pktgen client
# echo "Starting UDP pktgen client..."
# bin/examples/rust/udp-pktgen.elf --local "$CLIENT_HOST:$UDP_CLIENT_PORT" --remote "$SERVER_HOST:$UDP_SERVER_PORT" --bufsize $BUFFER_SIZE --injection_rate 10000
