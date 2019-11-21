#!/bin/bash -ex

# Undefined variables throw errors
set -u

SHREMOTE="../../Shremote/shremote.py"
CFG="dpdk_http_2.yml"

DO_CAT=0
DISABLE_PCM=0
N_REQS=100000
EXP_PREFIX=""
EXP_SUFFIX=""
BASE_OUTPUT="../../data/http/"

run_shremote() {
    ARGS="--rate $RATE --server-files $SERVER_FILES --client-files $CLIENT_FILES "
    TITLE=$LABEL

    if [[ $REQ_TYPE == 1 ]]; then
        ARGS+="--popular-page-workers 1 --unpopular-page-workers 1 "
        ARGS+="--net-dispatch-policy HTTP_REQ_TYPE "
        TITLE+="_reqtype"
    else
        ARGS+="--http-workers $N_HTTP "
        TITLE+="_n$N_HTTP"
    fi

    if [[ $DO_CAT == 0 ]]; then
        ARGS="$ARGS --cat-action reset "
    else
        ARGS="$ARGS --cat-action 'set $DO_CAT'"
    fi

    if [[ $DISABLE_PCM == 1 ]]; then
        ARGS="$ARGS --no-pcm"
    fi

    bash -c "$SHREMOTE $CFG $TITLE --out "$OUTDIR" --delete -- $ARGS"
}

run_exp_set() {
    LABEL="${EXP_PREFIX}${FILE_SZ}${EXP_SUFFIX}"
    OUTDIR="${BASE_OUTPUT}/${EXP_DIR}"
    mkdir -p file_lists
    SERVER_FILES=file_lists/srv_$FILE_SZ.txt
    CLIENT_FILES=file_lists/clt_$FILE_SZ.txt

    ./make_files_lists.py $SERVER_FILES $CLIENT_FILES $FILE_SZ $N_SRCS $N_REQS

    REQ_TYPE=0 N_HTTP=1 run_shremote
    REQ_TYPE=0 N_HTTP=2 run_shremote
    REQ_TYPE=1 N_HTTP=0 run_shremote
}

run_all_sizes() {
    FILE_SZ='256' N_SRCS=7200 run_exp_set
    FILE_SZ='512' N_SRCS=3600  run_exp_set
    FILE_SZ='1k' N_SRCS=1800  run_exp_set
    FILE_SZ='2k' N_SRCS=900  run_exp_set
    FILE_SZ='4k' N_SRCS=450  run_exp_set
    FILE_SZ='8k' N_SRCS=224  run_exp_set
    FILE_SZ='16k' N_SRCS=112  run_exp_set
    FILE_SZ='32k' N_SRCS=56  run_exp_set
    FILE_SZ='64k' N_SRCS=28  run_exp_set
    FILE_SZ='128k' N_SRCS=14  run_exp_set
    FILE_SZ='256k' N_SRCS=6  run_exp_set
    FILE_SZ='512k' N_SRCS=2  run_exp_set
    FILE_SZ='768k' N_SRCS=2  run_exp_set
}

RATE=10000 EXP_DIR="trace_on" FILE_SZ='256' N_SRCS=7200 run_exp_set
#EXP_DIR="trace_on" RATE=10000 run_all_sizes
#EXP_DIR="body_size/rate_10000" RATE=10000 run_all_sizes

#EXP_DIR=rate_5000 RATE=5000 run_all_sizes
#EXP_DIR=rate_10000 RATE=10000 run_all_sizes
#EXP_DIR=rate_15000 RATE=15000 run_all_sizes

run_small_sizes() {

    BASE_EXP_DIR=small_sizes
    for RATE in ${RATES[@]}; do
        EXP_DIR="$BASE_EXP_DIR/rate_$RATE"
        FILE_SZ='256' N_SRCS=7200  run_exp_set
        FILE_SZ='512' N_SRCS=3600  run_exp_set
        FILE_SZ='1k' N_SRCS=1800  run_exp_set
        FILE_SZ='2k' N_SRCS=900  run_exp_set
    done
}

#RATES="25000 22500 20000 17500 15000 12500 10000" run_small_sizes

#FILE_SZ='8k' N_SRCS=224  EXP_SUFFIX=_test run_exp_set
#FILE_SZ='768k' N_SRCS=2  EXP_SUFFIX=_2 run_exp_set

#DO_CAT=0 FILE_SZ='512k' N_SRCS=50  EXP_PREFIX="l3_" run_exp_set

#DO_CAT=net_all FILE_SZ='512k' N_SRCS=10  EXP_PREFIX="net_all_cat_" run_exp_set
#DO_CAT='even' FILE_SZ='512k' N_SRCS=10  EXP_PREFIX="less_cat_" run_exp_set
#DO_CAT=0 FILE_SZ='512k' N_SRCS=10  EXP_PREFIX="less_nocat_" run_exp_set

#FILE_SZ='500' N_SRCS=3600  DISABLE_PCM=1 EXP_SUFFIX="_no_pcm" run_exp_set
#FILE_SZ='1k' N_SRCS=1800  DISABLE_PCM=1 EXP_SUFFIX="_no_pcm" run_exp_set
#FILE_SZ='2k' N_SRCS=900  DISABLE_PCM=1 EXP_SUFFIX="_no_pcm" run_exp_set
#FILE_SZ='4k' N_SRCS=450  DISABLE_PCM=1 EXP_SUFFIX="_no_pcm" run_exp_set
