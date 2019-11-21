#!/bin/bash -ex

# Undefined variables throw errors
set -u

SHREMOTE="../../Shremote/shremote.py"
CFG="kv.yml"

DO_CAT=0
USE_PCM=0
BASE_OUTPUT="../../data/kv/"
CMD_FILES_BASE="./cmd_files"

run_shremote() {
    ARGS="--client-cmds ${CLIENT_CMDS} --server-cmds ${SERVER_CMDS}"
    ARGS="$ARGS --n-workers ${N_WORKERS} --pipeline ${PIPELINE} --strategy ${STRATEGY}"
    TITLE=$LABEL

    if [[ $DO_CAT == 0 ]]; then
        ARGS="$ARGS --cat-action reset "
    else
        ARGS="$ARGS --cat-action 'set $DO_CAT'"
    fi

    if [[ $USE_PCM == 1 ]]; then
        ARGS="$ARGS --use-pcm"
    fi

    bash -c "$SHREMOTE $CFG $TITLE --out "$OUTDIR" --delete -- $ARGS"
}

worker_x_pipeline() {

    N_KEYS=$(( 900000 * ${OVERLOAD} / ${VALUE_SIZE}))
    BASE_LABEL=${N_KEYS}x${VALUE_SIZE}
    CMD_FILES_LOC=${CMD_FILES_BASE}/${BASE_LABEL}/
    mkdir -p $CMD_FILES_LOC
    CLIENT_CMDS=${CMD_FILES_LOC}/client_cmds
    SERVER_CMDS=${CMD_FILES_LOC}/server_cmds

    N_REQS=$(( $N_KEYS * 200 ))

    ./make_cmd_lists.py $SERVER_CMDS $CLIENT_CMDS $VALUE_SIZE $N_KEYS $N_REQS SZOF

    for N_WORKERS in 1 2 3 4; do
        for PIPELINE in 1 2 4 8; do
            for STRATEGY in RR KEY; do
                LABEL="w${N_WORKERS}_p${PIPELINE}" OUTDIR="${BASE_OUTPUT}/${STRATEGY}/${BASE_LABEL}" run_shremote
            done
        done
    done
}

#BASE_OUTPUT=${BASE_OUTPUT}/szof
#for OVERLOAD in 1 2 3 4; do
#    for VALUE_SIZE in 500 1000 4000 16000 64000 256000 512000; do
#        worker_x_pipeline
#    done
#done

overload_equals_worker() {
    for N_WORKERS in 2 3 4 5; do
        N_KEYS=$(( 900000 * ${N_WORKERS} / ${VALUE_SIZE} ))
        BASE_LABEL=${N_KEYS}x${VALUE_SIZE}
        CMD_FILES_LOC=${CMD_FILES_BASE}/${BASE_LABEL}
        mkdir -p ${CMD_FILES_LOC}
        CLIENT_CMDS=${CMD_FILES_LOC}/client_cmds
        SERVER_CMDS=${CMD_FILES_LOC}/server_cmds

        N_REQS=$(( $N_KEYS * 1000 ))

        ./make_cmd_lists.py ${SERVER_CMDS} ${CLIENT_CMDS} $VALUE_SIZE $N_KEYS $N_REQS $REQTYPE

        for PIPELINE in $PIPELINES; do
            for STRATEGY in RR KEY; do
                LABEL="w${N_WORKERS}_p${PIPELINE}" OUTDIR="${BASE_OUTPUT}/${STRATEGY}/${BASE_LABEL}" run_shremote
            done
        done
    done
}

#BASE_OUTPUT=${BASE_OUTPUT}/store_dump/
#REQTYPE=SZOF PIPELINES=1 VALUE_SIZE=500 overload_equals_worker


#BASE_OUTPUT=${BASE_OUTPUT}/nnz/
#for VALUE_SIZE in 512000 128000 32000 8000 2000 500; do
#    REQTYPE=NNZ overload_equals_worker;
#done

#BASE_OUTPUT=${BASE_OUTPUT}/nettrace/szof/
#PIPELINES="1 2 3 4"
#for VALUE_SIZE in 768000 512000 128000 32000 8000 2000 500; do
#    REQTYPE=SZOF overload_equals_worker;
#done

run_test() {
    BASE_LABEL="500b"
    CMD_FILES_LOC=${CMD_FILES_BASE}/${BASE_LABEL}
    CLIENT_CMDS=${CMD_FILES_LOC}/client_cmds
    SERVER_CMDS=${CMD_FILES_LOC}/server_cmds
    VALUE_SIZE=500
    N_WORKERS=2
    N_KEYS=$(( 900000 * ${N_WORKERS} / ${VALUE_SIZE} ))
    N_REQS=$(( $N_KEYS * 1000 ))
    REQTYPE=GET
    mkdir -p $CMD_FILES_LOC
    ./make_cmd_lists.py ${SERVER_CMDS} ${CLIENT_CMDS} $VALUE_SIZE $N_KEYS $N_REQS $REQTYPE
    LABEL="w2_p1_after_choice_GET_${STRATEGY}" OUTDIR="${BASE_OUTPUT}/${BASE_LABEL}" PIPELINE=1 run_shremote
}

#STRATEGY=KEY run_test
#STRATEGY=RR run_test


one_overload_equals_worker() {
    N_KEYS=$(( 900000 * ${N_WORKERS} / ${VALUE_SIZE} ))
    BASE_LABEL=${N_KEYS}x${VALUE_SIZE}
    CMD_FILES_LOC=${CMD_FILES_BASE}/${BASE_LABEL}
    mkdir -p ${CMD_FILES_LOC}
    CLIENT_CMDS=${CMD_FILES_LOC}/client_cmds
    SERVER_CMDS=${CMD_FILES_LOC}/server_cmds

    N_REQS=$(( $N_KEYS * 1000 ))

    ./make_cmd_lists.py ${SERVER_CMDS} ${CLIENT_CMDS} $VALUE_SIZE $N_KEYS $N_REQS $REQTYPE

    for PIPELINE in $PIPELINES; do
        for STRATEGY in RR KEY; do
            LABEL="w${N_WORKERS}_p${PIPELINE}" OUTDIR="${BASE_OUTPUT}/${STRATEGY}/${BASE_LABEL}" run_shremote
        done
    done
}

BASE_OUTPUT=${BASE_OUTPUT}/store_dump_pcm/
USE_PCM=1 REQTYPE=SZOF PIPELINES=1 VALUE_SIZE=500 N_WORKERS=5 one_overload_equals_worker

BASE_OUTPUT=${BASE_OUTPUT}/store_dump_pcm/
USE_PCM=1 REQTYPE=SZOF PIPELINES=1 VALUE_SIZE=500 N_WORKERS=4 one_overload_equals_worker
