# Copyright (c) Microsoft Corporation.
# Licensed under the MIT license.

#===============================================================================
# System Directories
#===============================================================================

export PREFIX ?= $(HOME)
export INSTALL_PREFIX ?= $(HOME)
export PKG_CONFIG_PATH ?= $(shell find $(PREFIX)/lib/ -name '*pkgconfig*' -type d | xargs | sed -e 's/\s/:/g')
export LD_LIBRARY_PATH ?= $(HOME)/lib:$(shell find $(PREFIX)/lib/ -name '*x86_64-linux-gnu*' -type d | xargs | sed -e 's/\s/:/g')

#===============================================================================
# Project Directories
#===============================================================================

export BINDIR ?= $(CURDIR)/bin
export INCDIR ?= $(CURDIR)/include
export SRCDIR = $(CURDIR)/src

#===============================================================================
# Toolchain Configuration
#===============================================================================

# Rust
export BUILD ?= --release
export CARGO ?= $(HOME)/.cargo/bin/cargo
export CARGO_FLAGS ?=

#===============================================================================
# Libraries
#===============================================================================

ifeq ($(BUILD),--release)
export DEMIKERNEL_LIB := $(CURDIR)/target/release/libdemikernel.so
else
export DEMIKERNEL_LIB := $(CURDIR)/target/debug/libdemikernel.so
endif
export LIBS := $(DEMIKERNEL_LIB)

#===============================================================================
# Build Parameters
#===============================================================================

export LIBOS ?= catnap
export CARGO_FEATURES := --features=$(LIBOS)-libos

ifeq ($(LIBOS),catnip)
DRIVER ?= $(shell [ ! -z "`lspci | grep -E "ConnectX-[4,5]"`" ] && echo mlx5 || echo mlx4)
CARGO_FEATURES += --features=$(DRIVER)
endif

export PROFILER=no
ifeq ($(PROFILER),yes)
CARGO_FEATURES += --features=profiler
endif

CARGO_FEATURES += $(FEATURES)

#===============================================================================

all: all-libs all-tests

all-libs: check-fmt
	@echo "$(CARGO) build $(BUILD) $(CARGO_FEATURES) $(CARGO_FLAGS)"
	$(CARGO) build $(BUILD) $(CARGO_FEATURES) $(CARGO_FLAGS)

all-tests: all-tests-rust all-tests-c

all-tests-rust: make-dirs all-libs
	@echo "$(CARGO) build  --tests $(BUILD) $(CARGO_FEATURES) $(CARGO_FLAGS)"
	$(CARGO) build  --tests $(BUILD) $(CARGO_FEATURES) $(CARGO_FLAGS)

all-tests-c: make-dirs all-libs
	$(MAKE) -C tests all

check-fmt: check-fmt-c check-fmt-rust

check-fmt-c:
	$(shell find include/ -name "*.h" -name "*.hxx" -name "*.c" -name "*.cpp" -type f -print0 | xargs -0 clang-format --fallback-style=Microsoft --dry-run -Werror )
	@exit $(.SHELLSTATUS)

check-fmt-rust:
	$(CARGO) fmt -- --check

# Copies demikernel artifacts to a INSTALL_PREFIX directory.
install:
	mkdir -p $(INSTALL_PREFIX)/include $(INSTALL_PREFIX)/lib
	cp -rf $(INCDIR)/* $(INSTALL_PREFIX)/include/
	cp -f  $(DEMIKERNEL_LIB) $(INSTALL_PREFIX)/lib/

make-dirs:
	mkdir -p $(BINDIR)

clean: clean-rust clean-c

clean-rust:
	rm -rf target ; \
	rm -f Cargo.lock ; \
	$(CARGO) clean

clean-c:
	$(MAKE) -C tests clean

#===============================================================================

export CONFIG_PATH ?= $(HOME)/config.yaml
export MTU ?= 1500
export MSS ?= 1500
export PEER ?= server
export TEST ?= udp_push_pop
export TIMEOUT ?= 30

# Runs system tests.
test-system: test-system-rust

# Rust system tests.
test-system-rust: all-tests-rust
	timeout $(TIMEOUT) $(CARGO) test $(BUILD) $(CARGO_FEATURES) $(CARGO_FLAGS) -- --nocapture $(TEST)

# Runs unit tests.
test-unit: test-unit-rust

# C unit tests.
test-unit-c: all-tests-c
	@for f in $(shell ls $(BINDIR)); do $(BINDIR)/$${f}; done

# Rust unit tests.
# TODO: Find out a way of launching all unit tests without having to explicity state all of them.
test-unit-rust: test-unit-c
	$(CARGO) test $(BUILD) $(CARGO_FEATURES) -- --nocapture --test-threads=1 test_unit_sga_alloc_free_single_small
	$(CARGO) test $(BUILD) $(CARGO_FEATURES) -- --nocapture --test-threads=1 test_unit_sga_alloc_free_loop_tight_small
	$(CARGO) test $(BUILD) $(CARGO_FEATURES) -- --nocapture --test-threads=1 test_unit_sga_alloc_free_loop_decoupled_small
	$(CARGO) test $(BUILD) $(CARGO_FEATURES) -- --nocapture --test-threads=1 test_unit_sga_alloc_free_single_big
	$(CARGO) test $(BUILD) $(CARGO_FEATURES) -- --nocapture --test-threads=1 test_unit_sga_alloc_free_loop_tight_big
	$(CARGO) test $(BUILD) $(CARGO_FEATURES) -- --nocapture --test-threads=1 test_unit_sga_alloc_free_loop_decoupled_big
