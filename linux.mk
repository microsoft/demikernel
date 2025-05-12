# Copyright (c) Microsoft Corporation.
# Licensed under the MIT license.

#=======================================================================================================================
# Default Paths
#=======================================================================================================================

export PREFIX ?= $(HOME)
export INSTALL_PREFIX ?= $(HOME)
export PKG_CONFIG_PATH ?= $(shell find $(PREFIX)/lib/ -name '*pkgconfig*' -type d 2> /dev/null | xargs | sed -e 's/\s/:/g')
export LD_LIBRARY_PATH ?= $(CURDIR)/lib:$(shell find $(PREFIX)/lib/ -name '*x86_64-linux-gnu*' -type d 2> /dev/null | xargs | sed -e 's/\s/:/g')

#=======================================================================================================================
# Build Configuration
#=======================================================================================================================

export BUILD := release
ifeq ($(DEBUG),yes)
export RUST_LOG ?= trace
export BUILD := dev
endif

#=======================================================================================================================
# Project Directories
#=======================================================================================================================

export BINDIR ?= $(CURDIR)/bin
export INCDIR := $(CURDIR)/include
export LIBDIR ?= $(CURDIR)/lib
export SRCDIR = $(CURDIR)/src
export BUILD_DIR := $(CURDIR)/target/release
ifeq ($(BUILD),dev)
export BUILD_DIR := $(CURDIR)/target/debug
endif
export INPUT ?= $(CURDIR)/network_simulator/input

#=======================================================================================================================
# Toolchain Configuration
#=======================================================================================================================

# Rust
export CARGO ?= $(shell which cargo || echo "$(HOME)/.cargo/bin/cargo" )
export CARGO_FLAGS += --profile $(BUILD)
export RUSTFLAGS ?= -D warnings

# C
export CFLAGS := -I $(INCDIR)
ifeq ($(DEBUG),yes)
export CFLAGS += -O0
else
export CFLAGS += -O3
endif

#=======================================================================================================================
# Libraries
#=======================================================================================================================

export DEMIKERNEL_LIB := libdemikernel.so
export LIBS := $(BUILD_DIR)/$(DEMIKERNEL_LIB)

#=======================================================================================================================
# Build Parameters
#=======================================================================================================================

export LIBOS ?= catnap
export CARGO_FEATURES := --features=$(LIBOS)-libos --no-default-features

# Switch for DPDK
ifeq ($(LIBOS),catnip)
DRIVER ?= $(shell [ ! -z "`lspci | grep -E "ConnectX-[4,5,6]"`" ] && echo mlx5 || echo mlx4)
CARGO_FEATURES += --features=$(DRIVER)
endif

export PROFILER ?= no
ifeq ($(PROFILER),yes)
CARGO_FEATURES += --features=profiler
endif

export DIRECT_MAPPING ?= not-set
ifeq ($(DIRECT_MAPPING),yes)
CARGO_FEATURES += --features=direct-mapping
else ifneq ($(DIRECT_MAPPING),no)
ifeq ($(DEBUG),no)
CARGO_FEATURES += --features=direct-mapping
endif
endif

CARGO_FEATURES += $(FEATURES)

#=======================================================================================================================
# Targets
#=======================================================================================================================

all: init | all-libs all-tests all-examples

init:
	mkdir -p $(LIBDIR)
	git config --local core.hooksPath .githooks

doc:
	$(CARGO) doc $(FLAGS) --no-deps

# Copies demikernel artifacts to a INSTALL_PREFIX directory.
install:
	mkdir -p $(INSTALL_PREFIX)/include $(INSTALL_PREFIX)/lib
	cp -rf $(INCDIR)/* $(INSTALL_PREFIX)/include/
	cp -rf  $(LIBDIR)/* $(INSTALL_PREFIX)/lib/
	cp -f $(CURDIR)/scripts/config/default.yaml $(INSTALL_PREFIX)/config.yaml

#=======================================================================================================================
# Libs
#=======================================================================================================================

all-libs: all-libs-demikernel

all-libs-demikernel:
	@echo "LD_LIBRARY_PATH: $(LD_LIBRARY_PATH)"
	@echo "PKG_CONFIG_PATH: $(PKG_CONFIG_PATH)"
	$(CARGO) build --lib $(CARGO_FEATURES) $(CARGO_FLAGS)
	cp -f $(BUILD_DIR)/$(DEMIKERNEL_LIB) $(LIBDIR)/$(DEMIKERNEL_LIB)

clean-libs: clean-libs-demikernel

clean-libs-demikernel:
	rm -f $(LIBDIR)/$(DEMIKERNEL_LIB)
	rm -rf target ; \
	rm -f Cargo.lock ; \
	$(CARGO) clean

#=======================================================================================================================
# Tests
#=======================================================================================================================

all-tests: all-tests-rust all-tests-c

all-tests-rust:
	$(CARGO) build --tests $(CARGO_FEATURES) $(CARGO_FLAGS)

all-tests-c: all-libs
	$(MAKE) -C tests all

clean-tests: clean-tests-c

clean-tests-c:
	$(MAKE) -C tests clean

#=======================================================================================================================
# Examples
#=======================================================================================================================

all-examples: all-examples-c all-examples-rust

all-examples-c: all-libs
	$(MAKE) -C examples/c all

all-examples-rust:
	$(MAKE) -C examples/rust all

clean-examples: clean-examples-c clean-examples-rust

clean-examples-c:
	$(MAKE) -C examples/c clean

clean-examples-rust:
	$(MAKE) -C examples/rust clean

#=======================================================================================================================
# Benchmarks
#=======================================================================================================================

bench:
	$(CARGO) bench $(CARGO_FEATURES) $(CARGO_FLAGS) -- --nocapture

all-benchmarks-c: all-libs
	$(MAKE) -C benchmarks all

clean-benchmarks-c:
	$(MAKE) -C benchmarks clean

#=======================================================================================================================
# Code formatting
#=======================================================================================================================

check-fmt: check-fmt-c check-fmt-rust

check-fmt-c:
	$(shell find include/ -name "*.h" -name "*.hxx" -name "*.c" -name "*.cpp" -type f -print0 | xargs -0 clang-format --fallback-style=Microsoft --dry-run -Werror )
	@exit $(.SHELLSTATUS)

check-fmt-rust:
	$(CARGO) fmt --all -- --check

#=======================================================================================================================
# Clean
#=======================================================================================================================

clean: clean-examples clean-tests clean-libs

#=======================================================================================================================
# Tests
#=======================================================================================================================

export CONFIG_PATH ?= $(HOME)/config.yaml
export MTU ?= 1500
export PEER ?= server
export TEST ?= udp-push-pop
export TEST_INTEGRATION ?= tcp-tests
export TEST_UNIT ?=
export TIMEOUT_SECONDS ?= 512

test-system: test-system-rust

test-system-rust:
	timeout $(TIMEOUT_SECONDS) $(BINDIR)/examples/rust/$(TEST).elf $(ARGS)

test-unit: test-unit-rust

test-unit-c: all-tests test-unit-c-sizes test-unit-c-syscalls

test-unit-c-sizes: all-tests $(BINDIR)/sizes.elf
	timeout $(TIMEOUT_SECONDS) $(BINDIR)/sizes.elf

test-unit-c-syscalls: all-tests $(BINDIR)/syscalls.elf
	timeout $(TIMEOUT_SECONDS) $(BINDIR)/syscalls.elf

test-unit-rust: test-unit-rust-lib test-unit-rust-udp test-unit-rust-tcp
	timeout $(TIMEOUT_SECONDS) $(CARGO) test --test sga $(BUILD) $(CARGO_FEATURES) -- --nocapture --test-threads=1 test_unit_sga_alloc_free_single_small
	timeout $(TIMEOUT_SECONDS) $(CARGO) test --test sga $(BUILD) $(CARGO_FEATURES) -- --nocapture --test-threads=1 test_unit_sga_alloc_free_loop_tight_small
	timeout $(TIMEOUT_SECONDS) $(CARGO) test --test sga $(BUILD) $(CARGO_FEATURES) -- --nocapture --test-threads=1 test_unit_sga_alloc_free_loop_decoupled_small
	timeout $(TIMEOUT_SECONDS) $(CARGO) test --test sga $(BUILD) $(CARGO_FEATURES) -- --nocapture --test-threads=1 test_unit_sga_alloc_free_single_big
	timeout $(TIMEOUT_SECONDS) $(CARGO) test --test sga $(BUILD) $(CARGO_FEATURES) -- --nocapture --test-threads=1 test_unit_sga_alloc_free_loop_tight_big
	timeout $(TIMEOUT_SECONDS) $(CARGO) test --test sga $(BUILD) $(CARGO_FEATURES) -- --nocapture --test-threads=1 test_unit_sga_alloc_free_loop_decoupled_big

test-unit-rust-lib: all-tests-rust
	timeout $(TIMEOUT_SECONDS) $(CARGO) test --lib $(CARGO_FLAGS) $(CARGO_FEATURES) -- --nocapture $(TEST_UNIT)

test-unit-rust-udp: all-tests-rust
	timeout $(TIMEOUT_SECONDS) $(CARGO) test --test udp $(CARGO_FLAGS) $(CARGO_FEATURES) -- --nocapture $(TEST_UNIT)

test-unit-rust-tcp: all-tests-rust
	timeout $(TIMEOUT_SECONDS) $(CARGO) test --test tcp $(CARGO_FLAGS) $(CARGO_FEATURES) -- --nocapture $(TEST_UNIT)

test-integration-rust:
	timeout $(TIMEOUT_SECONDS) $(CARGO) test --test $(TEST_INTEGRATION) $(CARGO_FLAGS) $(CARGO_FEATURES) -- $(ARGS)

test-clean:
	rm -f /dev/shm/demikernel-*

#=======================================================================================================================
# Benchmarks
#=======================================================================================================================

run-benchmarks-c: all-benchmarks-c $(BINDIR)/syscalls.elf
	timeout $(TIMEOUT_SECONDS) $(BINDIR)/benchmarks.elf
