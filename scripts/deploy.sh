#!/bin/bash

# Copyright (c) Microsoft Corporation.
# Licensed under the MIT license.

# Mandatory Arguments
REMOTE_HOST=$1
REMOTE_PATH=$2

# Optional Arguments
MAKE_FLAGS=$3
MAKE_TARGET=$4

# SSH Options
SSH_OPTIONS=""

# RSYNC Options
RSYNC_OPTIONS="-avz --delete-after"
RSYNC_OPTIONS="$RSYNC_OPTIONS --exclude=.git"
RSYNC_OPTIONS="$RSYNC_OPTIONS --exclude='*.swp'"
RSYNC_OPTIONS="$RSYNC_OPTIONS --exclude=target"
RSYNC_OPTIONS="$RSYNC_OPTIONS --exclude='*.lock'"

#===============================================================================

#
# Prints script usage and exits.
#
function usage {
	echo "Usage: deploy.sh remote-host remote-path [make-flags] [make-target]"
	exit 0
}

#===============================================================================

set -e

# Check for mandatory arguments
if [ -z $REMOTE_HOST ];
then
	echo "missing remote host"
	usage
fi
if [ -z $REMOTE_PATH ];
then
	echo "missing remote host"
	usage
fi

ssh $SSH_OPTIONS $REMOTE_HOST "bash -l -c 'mkdir -p $REMOTE_PATH'"
rsync $RSYNC_OPTIONS . "$REMOTE_HOST":"$REMOTE_PATH"
ssh $SSH_OPTIONS $REMOTE_HOST "bash -l -c 'cd $REMOTE_PATH && make $MAKE_FLAGS $MAKE_TARGET 2>&1 | tee ~/\$HOSTNAME'"
