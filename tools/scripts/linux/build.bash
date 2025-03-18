#!/bin/bash

set -e

# Default parameters for build configuration
CONFIG="release"
LIBOS="catnap"
PROFILER="no"

# Validate and parse command-line arguments
while [[ $# -gt 0 ]]; do
    case "$1" in
        --config)
            # Validate build configuration argument
            if [[ "$2" != "debug" && "$2" != "release" ]]; then
                echo "Invalid value for --config. Allowed values: debug, release"
                exit 1
            fi
            CONFIG="$2"
            shift 2
            ;;
        --libos)
            # Validate LibOS selection argument
            if [[ "$2" != "catnap" && "$2" != "catnip" && "$2" != "catpowder" ]]; then
                echo "Invalid value for --libos. Allowed values: catnap, catnip, catpowder"
                exit 1
            fi
            LIBOS="$2"
            shift 2
            ;;
        --profiler)
            # Enable profiler support
            PROFILER="yes"
            shift
            ;;
        *)
            # Handle unknown arguments
            echo "Unknown argument: $1"
            exit 1
            ;;
    esac
done

export RUSTC_BOOTSTRAP=1
export RUST_LOG="trace"
export RUST_BACKTRACE="full"
export LIBOS="$LIBOS"

ARGS=""
if [[ "$CONFIG" == "debug" ]]; then
    ARGS+="DEBUG=yes "
fi
if [[ "$PROFILER" == "yes" ]]; then
    ARGS+="PROFILER=yes "
else
    ARGS+="PROFILER=no "
fi

COMMAND="${ARGS}all"

echo "Using LibOS: $LIBOS; running 'make $COMMAND' to build Demikernel"
echo ""

# Execute the build command
make $COMMAND
