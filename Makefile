# Copyright (c) Microsoft Corporation.
# Licensed under the MIT license.

export PREFIX ?= $(HOME)

export PKG_CONFIG_PATH ?= $(shell find $(PREFIX)/lib/ -name '*pkgconfig*' -type d)
export LD_LIBRARY_PATH ?= $(shell find $(PREFIX)/lib/ -name '*x86_64-linux-gnu*' -type d)
export CONFIG_PATH ?= $(HOME)/config.yaml

export MTU ?= 1500
export MSS ?= 9000
export PEER ?= server
export TEST ?= udp_push_pop

export TIMEOUT ?= 30

#===============================================================================
# Directories
#===============================================================================

export SRCDIR = $(CURDIR)/src
export BINDIR = $(CURDIR)/bin
export LIBDIR = $(CURDIR)/lib

#===============================================================================
# Toolchain Configuration
#===============================================================================

# Rust Toolchain
export BUILD ?= --release
export CARGO ?= $(HOME)/.cargo/bin/cargo

#===============================================================================

export DRIVER ?= $(shell [ ! -z "`lspci | grep -E "ConnectX-[4,5]"`" ] && echo mlx5 || echo mlx4)

#===============================================================================

all: all-libs all-tests

all-libs: all-libs-catnap all-libs-catnip

all-libs-default:
	$(CARGO) build --lib $(BUILD) $(CARGO_FLAGS)

all-libs-catnap: all-libs-default
	$(CARGO) build --features=catnap-libos $(BUILD) $(CARGO_FLAGS)

all-libs-catnip: all-libs-default
	$(CARGO) build --features=catnip-libos --features=$(DRIVER) $(BUILD) $(CARGO_FLAGS)

all-tests: all-tests-catnap all-tests-catnip

all-tests-default:
	$(CARGO) build --tests $(BUILD) $(CARGO_FLAGS)

all-tests-catnap: all-tests-default
	$(CARGO) build --tests --features=catnap-libos $(BUILD) $(CARGO_FLAGS)

all-tests-catnip: all-tests-default
	$(CARGO) build --tests --features=catnip-libos --features=$(DRIVER) $(BUILD) $(CARGO_FLAGS)

clean:
	rm -rf target &&  \
	$(CARGO) clean && \
	rm -f Cargo.lock

#===============================================================================

test-catnip: all-tets-catnip
	sudo -E LD_LIBRARY_PATH="$(LD_LIBRARY_PATH)" timeout $(TIMEOUT) $(CARGO) test $(BUILD) --features=$(DRIVER) $(CARGO_FLAGS) -p catnip-libos -- --nocapture $(TEST)

test-catnap: all-tests-catnap
	timeout $(TIMEOUT) $(CARGO) test $(CARGO_FLAGS) --features=catnap-libos -- --nocapture $(TEST)