# Copyright (c) Microsoft Corporation.
# Licensed under the MIT license.

#===============================================================================
# System Directories
#===============================================================================

export PREFIX ?= $(HOME)
export PKG_CONFIG_PATH ?= $(shell find $(PREFIX)/lib/ -name '*pkgconfig*' -type d)
export LD_LIBRARY_PATH ?= $(shell find $(PREFIX)/lib/ -name '*x86_64-linux-gnu*' -type d)

#===============================================================================
# Project Directories
#===============================================================================

export SRCDIR = $(CURDIR)/src

#===============================================================================
# Toolchain Configuration
#===============================================================================

export VERBOSE = yes
ifeq ($(VERBOSE),yes)
export CARGO_VERBOSE = --nocapture
endif

export BUILD ?= --release
export CARGO ?= $(HOME)/.cargo/bin/cargo
export CARGO_FLAGS ?=

#===============================================================================
# Build Parameters
#===============================================================================

export LIBOS ?= catnap
export CARGO_FEATURES := --features=$(LIBOS)-libos

ifeq ($(LIBOS),catnip)
DRIVER ?= $(shell [ ! -z "`lspci | grep -E "ConnectX-[4,5]"`" ] && echo mlx5 || echo mlx4)
CARGO_FEATURES += --features=$(DRIVER)
endif

#===============================================================================

all: all-libs all-tests

all-libs:
	$(CARGO) build $(BUILD) $(CARGO_FEATURES) $(CARGO_FLAGS)

all-tests:
	$(CARGO) build  --tests $(BUILD) $(CARGO_FEATURES) $(CARGO_FLAGS)

clean:
	rm -rf target ; \
	rm -f Cargo.lock ; \
	$(CARGO) clean

#===============================================================================

export CONFIG_PATH ?= $(HOME)/config.yaml
export MTU ?= 1500
export MSS ?= 9000
export PEER ?= server
export TEST ?= udp_push_pop
export TIMEOUT ?= 30

test-system: all-tests
	timeout $(TIMEOUT) $(CARGO) test $(BUILD) $(CARGO_FEATURES) $(CARGO_FLAGS) -- $(CARGO_VERBOSE) $(TEST)

test-unit:
	$(CARGO) test $(BUILD) $(CARGO_FEATURES) -- $(CARGO_VERBOSE) --test-threads=1 test_unit_getaddrinfo