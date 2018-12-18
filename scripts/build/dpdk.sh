#!/bin/bash

POSITIONAL=()
while [[ $# -gt 0 ]]
do
key="$1"

case $key in
    -s|--source-path)
    SOURCE_PATH="$2"
    shift # past argument
    shift # past value
    ;;
    -t|--target)
    TARGET="$2"
    shift # past argument
    shift # past value
    ;;
    -l|--lib)
    LIBPATH="$2"
    shift # past argument
    shift # past value
    ;;
    --config)
    CONFIG=yes
    shift # past argument
    ;;
    --debug)
    DEBUG=yes
    shift # past argument
    ;;
    *)    # unknown option
    POSITIONAL+=("$1") # save it in an array for later
    shift # past argument
    ;;
esac
done
set -- "${POSITIONAL[@]}" # restore positional parameters

if [ "x$CONFIG" = "xyes" ]; then
    config='config'
fi

if [ "x$DEBUG" = "xyes" ]; then
    EXTRA_CFLAGS='-O0 -g3' make -C $SOURCE_PATH $config T=$TARGET
else
    make -C $SOURCE_PATH $config T=$TARGET
fi

