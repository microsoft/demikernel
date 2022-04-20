# Copyright (c) Microsoft Corporation.
# Licensed under the MIT license.

#===============================================================================
# System Directories
#===============================================================================

export PREFIX ?= $(HOME)
export PKG_CONFIG_PATH ?= $(shell find $(PREFIX)/lib/ -name '*pkgconfig*' -type d | xargs | sed -e 's/\s/:/g')
export LD_LIBRARY_PATH ?= $(shell find $(PREFIX)/lib/ -name '*x86_64-linux-gnu*' -type d | xargs | sed -e 's/\s/:/g')

#===============================================================================
# Project Directories
#===============================================================================

export SRCDIR = $(CURDIR)/src

#===============================================================================
# Toolchain Configuration
#===============================================================================

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

all-libs: check-fmt
	$(CARGO) build $(BUILD) $(CARGO_FEATURES) $(CARGO_FLAGS)

all-tests: check-fmt
	$(CARGO) build  --tests $(BUILD) $(CARGO_FEATURES) $(CARGO_FLAGS)

check-fmt: check-fmt-c check-fmt-rust

check-fmt-c:
	$(shell find include/ -name "*.h" -name "*.hxx" -name "*.c" -name "*.cpp" -type f -print0 | xargs -0 clang-format --fallback-style=Microsoft --dry-run -Werror )
	@exit $(.SHELLSTATUS)

check-fmt-rust:
	$(CARGO) fmt -- --check

clean:
	rm -rf target ; \
	rm -f Cargo.lock ; \
	$(CARGO) clean

#===============================================================================

export CONFIG_PATH ?= $(HOME)/config.yaml
export MTU ?= 1500
export MSS ?= 1500
export PEER ?= server
export TEST ?= udp_push_pop
export TIMEOUT ?= 30

# Runs system tests.
test-system: all-tests
	timeout $(TIMEOUT) $(CARGO) test $(BUILD) $(CARGO_FEATURES) $(CARGO_FLAGS) -- --nocapture $(TEST)

# Runs unit tests.
# TODO: Find out a way of launching all unit tests without having to explicity state all of them.
test-unit:
	$(CARGO) test $(BUILD) $(CARGO_FEATURES) -- --nocapture --test-threads=1 test_unit_sga_alloc_free_single_small
	$(CARGO) test $(BUILD) $(CARGO_FEATURES) -- --nocapture --test-threads=1 test_unit_sga_alloc_free_loop_tight_small
	$(CARGO) test $(BUILD) $(CARGO_FEATURES) -- --nocapture --test-threads=1 test_unit_sga_alloc_free_loop_decoupled_small
	$(CARGO) test $(BUILD) $(CARGO_FEATURES) -- --nocapture --test-threads=1 test_unit_sga_alloc_free_single_big
	$(CARGO) test $(BUILD) $(CARGO_FEATURES) -- --nocapture --test-threads=1 test_unit_sga_alloc_free_loop_tight_big
	$(CARGO) test $(BUILD) $(CARGO_FEATURES) -- --nocapture --test-threads=1 test_unit_sga_alloc_free_loop_decoupled_big
