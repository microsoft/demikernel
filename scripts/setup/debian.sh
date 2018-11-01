#!/bin/sh

set -e

APT_PACKAGES="libgmp-dev libnuma-dev"

sudo apt-get -y install $APT_PACKAGES
