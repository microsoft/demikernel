#!/bin/bash

SHREMOTE='../Shremote/shremote.py'
CFG='dpdk_http.yml'
OUTPUT="$1"
SUFFIX="$2"
TEST_N="$3"

if [[ $SUFFIX == "" ]]; then
    echo "Usage: $0 OUTPUT_DIRECTORY SUFFIX";
    exit -1
fi

do_shre() {
    if [[ $TEST_N == "" || $TEST_N == $3 ]]; then
        $SHREMOTE $CFG $1_$SUFFIX --out $OUTPUT --delete --args "$2"
        if [[ $? != 0 ]]; then
            echo "FAILURE!!!!!"
            exit -1
        fi
    fi
}

do_shre all_pinned 'unpin_tcp:False;unpin_http:False' 1
do_shre tcp_unpinned 'unpin_tcp:True;unpin_http:False' 2
do_shre http_unpinned 'unpin_tcp:False;unpin_http:True' 3
do_shre no_pinned 'unpin_tcp:True;unpin_http:True' 4
