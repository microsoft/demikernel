#!/bin/bash

set -e

APT_PACKAGES="libevent-dev libevent-pthreads"

apt-get -y install $APT_PACKAGES
