# Copyright (c) Microsoft Corporation.
# Licensed under the MIT license.

#=======================================================================================================================
# Default Paths
#=======================================================================================================================

export PREFIX ?= $(HOME)
export INSTALL_PREFIX ?= $(HOME)
export PKG_CONFIG_PATH ?= $(shell find $(PREFIX)/lib/ -name '*pkgconfig*' -type d 2> /dev/null | xargs | sed -e 's/\s/:/g')
export LD_LIBRARY_PATH ?= $(HOME)/lib:$(shell find $(PREFIX)/lib/ -name '*x86_64-linux-gnu*' -type d 2> /dev/null | xargs | sed -e 's/\s/:/g')

#=======================================================================================================================
# Project Directories
#=======================================================================================================================

export BINDIR ?= $(CURDIR)/bin
export INCDIR ?= $(CURDIR)/include
export SRCDIR = $(CURDIR)/src

#=======================================================================================================================
# Toolchain Configuration
#=======================================================================================================================

export CARGO ?= $(HOME)/.cargo/bin/cargo

# Switches:
# - TEST    Test to run.
# - BENCH   Microbenchmark to run.
# - FLAGS   Flags passed to cargo.

# Set build mode.
ifneq ($(DEBUG),yes)
export BUILD = release
else
export BUILD = dev
endif
export CARGO_FLAGS += --profile $(BUILD)

#=======================================================================================================================
# Libraries
#=======================================================================================================================

ifeq ($(BUILD),release)
export DEMIKERNEL_LIB := $(CURDIR)/target/release/libdemikernel.so
else
export DEMIKERNEL_LIB := $(CURDIR)/target/debug/libdemikernel.so
endif
export LIBS := $(DEMIKERNEL_LIB)

#=======================================================================================================================
# Build Parameters
#=======================================================================================================================

export LIBOS ?= catnap
export CARGO_FEATURES := --features=$(LIBOS)-libos

# Switch for DPDK
ifeq ($(LIBOS),catnip)
DRIVER ?= $(shell [ ! -z "`lspci | grep -E "ConnectX-[4,5]"`" ] && echo mlx5 || echo mlx4)
CARGO_FEATURES += --features=$(DRIVER)
endif

# Switch for profiler.
export PROFILER=no
ifeq ($(PROFILER),yes)
CARGO_FEATURES += --features=profiler
endif

CARGO_FEATURES += $(FEATURES)

#=======================================================================================================================

all: all-libs all-tests

# Builds all libraries.
all-libs:
	@echo "$(CARGO) build --libs $(CARGO_FEATURES) $(CARGO_FLAGS)"
	$(CARGO) build --lib $(CARGO_FEATURES) $(CARGO_FLAGS)

# Builds regression tests.
all-tests: all-tests-rust all-tests-c

# Runs regression tests for Rust.
all-tests-rust: make-dirs all-libs
	@echo "$(CARGO) build  --tests $(CARGO_FEATURES) $(CARGO_FLAGS)"
	$(CARGO) build  --tests $(CARGO_FEATURES) $(CARGO_FLAGS)

# Runs regression tests for C.
all-tests-c: make-dirs all-libs
	$(MAKE) -C tests all

# Check code style formatting.
check-fmt: check-fmt-c check-fmt-rust

# Check code style formatting for C.
check-fmt-c:
	$(shell find include/ -name "*.h" -name "*.hxx" -name "*.c" -name "*.cpp" -type f -print0 | xargs -0 clang-format --fallback-style=Microsoft --dry-run -Werror )
	@exit $(.SHELLSTATUS)

# Check code style formatting for Rust.
check-fmt-rust:
	$(CARGO) fmt --all -- --check

# Builds documentation.
doc:
	$(CARGO) doc $(FLAGS) --no-deps

# Copies demikernel artifacts to a INSTALL_PREFIX directory.
install:
	mkdir -p $(INSTALL_PREFIX)/include $(INSTALL_PREFIX)/lib
	cp -rf $(INCDIR)/* $(INSTALL_PREFIX)/include/
	cp -f  $(DEMIKERNEL_LIB) $(INSTALL_PREFIX)/lib/

make-dirs:
	mkdir -p $(BINDIR)

# Cleans up all build artifacts.
clean: clean-rust clean-c

# Cleans up Rust build artifacts.
clean-rust:
	rm -rf target ; \
	rm -f Cargo.lock ; \
	$(CARGO) clean

# Cleans up C build artifacts.
clean-c:
	$(MAKE) -C tests clean

#=======================================================================================================================

export CONFIG_PATH ?= $(HOME)/config.yaml
export MTU ?= 1500
export MSS ?= 1500
export PEER ?= server
export TEST ?= udp_push_pop
export TIMEOUT ?= 30

# Runs system tests.
test-system: test-system-rust

# Rust system tests.
test-system-rust:
	timeout $(TIMEOUT) $(CARGO) test $(BUILD) $(CARGO_FEATURES) $(CARGO_FLAGS) -- --nocapture $(TEST)

# Runs unit tests.
test-unit: test-unit-rust

# C unit tests.
test-unit-c: $(BINDIR)/syscalls.elf
	$(BINDIR)/syscalls.elf

# Rust unit tests.
# TODO: Find out a way of launching all unit tests without having to explicity state all of them.
test-unit-rust: test-unit-c
	$(CARGO) test $(BUILD) $(CARGO_FEATURES) -- --nocapture --test-threads=1 test_unit_sga_alloc_free_single_small
	$(CARGO) test $(BUILD) $(CARGO_FEATURES) -- --nocapture --test-threads=1 test_unit_sga_alloc_free_loop_tight_small
	$(CARGO) test $(BUILD) $(CARGO_FEATURES) -- --nocapture --test-threads=1 test_unit_sga_alloc_free_loop_decoupled_small
	$(CARGO) test $(BUILD) $(CARGO_FEATURES) -- --nocapture --test-threads=1 test_unit_sga_alloc_free_single_big
	$(CARGO) test $(BUILD) $(CARGO_FEATURES) -- --nocapture --test-threads=1 test_unit_sga_alloc_free_loop_tight_big
	$(CARGO) test $(BUILD) $(CARGO_FEATURES) -- --nocapture --test-threads=1 test_unit_sga_alloc_free_loop_decoupled_big
