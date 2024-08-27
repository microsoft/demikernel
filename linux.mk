# Copyright (c) Microsoft Corporation.
# Licensed under the MIT license.

#=======================================================================================================================
# Default Paths
#=======================================================================================================================

export PREFIX ?= $(HOME)
export INSTALL_PREFIX ?= $(HOME)
export PKG_CONFIG_PATH ?= $(shell find $(PREFIX)/lib/ -name '*pkgconfig*' -type d 2> /dev/null | xargs | sed -e 's/\s/:/g')
export LD_LIBRARY_PATH = $(CURDIR)/lib:$(shell find $(PREFIX)/lib/ -name '*x86_64-linux-gnu*' -type d 2> /dev/null | xargs | sed -e 's/\s/:/g')

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
export INPUT ?= $(CURDIR)/nettest/input

#=======================================================================================================================
# Toolchain Configuration
#=======================================================================================================================

# Rust
export CARGO ?= $(shell which cargo || echo "$(HOME)/.cargo/bin/cargo" )
export CARGO_FLAGS += --profile $(BUILD)

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
export CARGO_FEATURES := --features=$(LIBOS)-libos,catnap-libos --no-default-features

# Switch for DPDK
ifeq ($(LIBOS),catnip)
DRIVER ?= $(shell [ ! -z "`lspci | grep -E "ConnectX-[4,5,6]"`" ] && echo mlx5 || echo mlx4)
CARGO_FEATURES += --features=$(DRIVER)
endif

# Switch for profiler.
export PROFILER ?= no
ifeq ($(PROFILER),yes)
CARGO_FEATURES += --features=profiler
endif

ifeq ($(TCP_MIG),1)
CARGO_FEATURES += --features=tcp-migration
endif

ifeq ($(CAPY_LOG),1)
CARGO_FEATURES += --features=capy-log
endif

CARGO_FEATURES += $(FEATURES)

#=======================================================================================================================

all: init | all-libs all-tests all-examples

init:
	mkdir -p $(LIBDIR)
	git config --local core.hooksPath .githooks

# Builds documentation.
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

# Builds all libraries.
all-libs: all-shim all-libs-demikernel

all-libs-demikernel:
	@echo "LD_LIBRARY_PATH: $(LD_LIBRARY_PATH)"
	@echo "PKG_CONFIG_PATH: $(PKG_CONFIG_PATH)"
	@echo "$(CARGO) build --libs $(CARGO_FEATURES) $(CARGO_FLAGS)"
	$(CARGO) build --lib $(CARGO_FEATURES) $(CARGO_FLAGS)
	cp -f $(BUILD_DIR)/$(DEMIKERNEL_LIB) $(LIBDIR)/$(DEMIKERNEL_LIB)

all-shim: all-libs-demikernel
	$(MAKE) -C shim all BINDIR=$(BINDIR)/shim

clean-libs: clean-shim clean-libs-demikernel

clean-libs-demikernel:
	rm -f $(LIBDIR)/$(DEMIKERNEL_LIB)
	rm -rf target ; \
	rm -f Cargo.lock ; \
	$(CARGO) clean

clean-shim:
	$(MAKE) -C shim clean BINDIR=$(BINDIR)/shim

#=======================================================================================================================
# Tests
#=======================================================================================================================

# Builds all tests.
all-tests: all-tests-rust all-tests-c

# Builds all Rust tests.
all-tests-rust:
	@echo "$(CARGO) build --tests $(CARGO_FEATURES) $(CARGO_FLAGS)"
	$(CARGO) build --tests $(CARGO_FEATURES) $(CARGO_FLAGS)

# Builds all C tests.
all-tests-c: all-libs
	$(MAKE) -C tests all

# Cleans up all build artifactos for tests.
clean-tests: clean-tests-c

# Cleans up all C build artifacts for tests.
clean-tests-c:
	$(MAKE) -C tests clean

#=======================================================================================================================
# Examples
#=======================================================================================================================

# Builds all examples.
all-examples: all-examples-c all-examples-rust

# Builds all C examples.
all-examples-c: all-libs
	$(MAKE) -C examples/c all

# Builds all Rust examples.
all-examples-rust:
	$(MAKE) -C examples/rust all

# Cleans all examples.
clean-examples: clean-examples-c clean-examples-rust

# Cleans all C examples.
clean-examples-c:
	$(MAKE) -C examples/c clean

# Cleans all Rust examples.
clean-examples-rust:
	$(MAKE) -C examples/rust clean

#=======================================================================================================================
# Benchmarks
#=======================================================================================================================

# Builds all C benchmarks
all-benchmarks-c: all-libs
	$(MAKE) -C benchmarks all

# Cleans up all C build artifacts for benchmarks.
clean-benchmarks-c:
	$(MAKE) -C benchmarks clean

#=======================================================================================================================
# Check
#=======================================================================================================================

# Check code style formatting.
check-fmt: check-fmt-c check-fmt-rust

# Check code style formatting for C.
check-fmt-c:
	$(shell find include/ -name "*.h" -name "*.hxx" -name "*.c" -name "*.cpp" -type f -print0 | xargs -0 clang-format --fallback-style=Microsoft --dry-run -Werror )
	@exit $(.SHELLSTATUS)

# Check code style formatting for Rust.
check-fmt-rust:
	$(CARGO) fmt --all -- --check

#=======================================================================================================================
# Clean
#=======================================================================================================================

# Cleans up all build artifacts.
clean: clean-examples clean-tests clean-libs

#=======================================================================================================================
# Tests
#=======================================================================================================================

export CONFIG_PATH ?= $(HOME)/config.yaml
export CONFIG_DIR = $(HOME)/Capybara/demikernel/scripts/config
export ELF_DIR ?= $(HOME)/Capybara/demikernel/bin/examples/rust
export MTU ?= 1500
export MSS ?= 1500
export PEER ?= server
export TEST ?= udp-push-pop
export TEST_INTEGRATION ?= tcp-test
export TEST_UNIT ?=
export TIMEOUT ?= 120

# Runs system tests.
test-system: test-system-rust

# Rust system tests.
test-system-rust:
	timeout $(TIMEOUT) $(BINDIR)/examples/rust/$(TEST).elf $(ARGS)

# Runs unit tests.
test-unit: test-unit-rust

# C unit tests.
test-unit-c: all-tests test-unit-c-sizes test-unit-c-syscalls

test-unit-c-sizes: all-tests $(BINDIR)/sizes.elf
	timeout $(TIMEOUT) $(BINDIR)/sizes.elf

test-unit-c-syscalls: all-tests $(BINDIR)/syscalls.elf
	timeout $(TIMEOUT) $(BINDIR)/syscalls.elf

# Rust unit tests.
test-unit-rust: test-unit-rust-lib test-unit-rust-udp test-unit-rust-tcp
	timeout $(TIMEOUT) $(CARGO) test --test sga $(BUILD) $(CARGO_FEATURES) -- --nocapture --test-threads=1 test_unit_sga_alloc_free_single_small
	timeout $(TIMEOUT) $(CARGO) test --test sga $(BUILD) $(CARGO_FEATURES) -- --nocapture --test-threads=1 test_unit_sga_alloc_free_loop_tight_small
	timeout $(TIMEOUT) $(CARGO) test --test sga $(BUILD) $(CARGO_FEATURES) -- --nocapture --test-threads=1 test_unit_sga_alloc_free_loop_decoupled_small
	timeout $(TIMEOUT) $(CARGO) test --test sga $(BUILD) $(CARGO_FEATURES) -- --nocapture --test-threads=1 test_unit_sga_alloc_free_single_big
	timeout $(TIMEOUT) $(CARGO) test --test sga $(BUILD) $(CARGO_FEATURES) -- --nocapture --test-threads=1 test_unit_sga_alloc_free_loop_tight_big
	timeout $(TIMEOUT) $(CARGO) test --test sga $(BUILD) $(CARGO_FEATURES) -- --nocapture --test-threads=1 test_unit_sga_alloc_free_loop_decoupled_big

# Rust unit tests for the library.
test-unit-rust-lib: all-tests-rust
	timeout $(TIMEOUT) $(CARGO) test --lib $(CARGO_FLAGS) $(CARGO_FEATURES) -- --nocapture $(TEST_UNIT)

# Rust unit tests for UDP.
test-unit-rust-udp: all-tests-rust
	timeout $(TIMEOUT) $(CARGO) test --test udp $(CARGO_FLAGS) $(CARGO_FEATURES) -- --nocapture $(TEST_UNIT)

# Rust unit tests for TCP.
test-unit-rust-tcp: all-tests-rust
	timeout $(TIMEOUT) $(CARGO) test --test tcp $(CARGO_FLAGS) $(CARGO_FEATURES) -- --nocapture $(TEST_UNIT)

# Runs Rust integration tests.
test-integration-rust:
	timeout $(TIMEOUT) $(CARGO) test --test $(TEST_INTEGRATION) $(CARGO_FLAGS) $(CARGO_FEATURES) -- $(ARGS)

# Cleans dangling test resources.
test-clean:
	rm -f /dev/shm/demikernel-*

#=======================================================================================================================
# Benchmarks
#=======================================================================================================================

# C unit benchmarks.
run-benchmarks-c: all-benchmarks-c $(BINDIR)/syscalls.elf
	timeout $(TIMEOUT) $(BINDIR)/benchmarks.elf

ENV += CAPY_LOG=all 
ENV += LIBOS=catnip
 
tcp-ping-pong-server8:
	sudo -E \
	CONFIG_PATH=$(CONFIG_DIR)/node8_config.yaml \
	$(ENV) \
	LD_LIBRARY_PATH=$(LD_LIBRARY_PATH) \
	taskset --cpu-list 0 numactl -m0 \
	$(ELF_DIR)/tcp-ping-pong.elf --server 10.0.1.8:10008

tcp-ping-pong-server9:
	sudo -E \
	CONFIG_PATH=$(CONFIG_DIR)/node9_config.yaml \
	$(ENV) \
	MIG_OFF=1 \
	LD_LIBRARY_PATH=$(LD_LIBRARY_PATH) \
	taskset --cpu-list 0 numactl -m0 \
	$(ELF_DIR)/tcp-ping-pong.elf --server 10.0.1.9:10009


tcp-ping-pong-client:
	sudo -E \
	CONFIG_PATH=$(CONFIG_DIR)/node7_config.yaml \
	$(ENV) \
	MIG_OFF=1 \
	LD_LIBRARY_PATH=$(LD_LIBRARY_PATH) \
	taskset --cpu-list 0 numactl -m0 \
	$(ELF_DIR)/tcp-ping-pong.elf --client 10.0.1.8:10000

redis-server-node8:
	cd ../capybara-redis/src && \
	sudo -E \
	$(ENV) \
	CONFIG_PATH=$(CONFIG_DIR)/node8_config.yaml \
	LD_LIBRARY_PATH=$(LD_LIBRARY_PATH) \
	LD_PRELOAD=$(LIBDIR)/libshim.so \
	./redis-server ../config/node8.conf

redis-server-node9:
	cd ../capybara-redis/src && \
	sudo -E \
	MIG_OFF=1 \
	$(ENV) \
	CONFIG_PATH=$(CONFIG_DIR)/node9_config.yaml \
	LD_LIBRARY_PATH=$(LD_LIBRARY_PATH) \
	LD_PRELOAD=$(LIBDIR)/libshim.so \
	./redis-server ../config/node9.conf


prometheus-ping-pong-client:
	sudo -E \
	CONFIG_PATH=$(CONFIG_DIR)/p40_config.yaml \
	$(ENV) \
	MIG_OFF=1 \
	LD_LIBRARY_PATH=$(LD_LIBRARY_PATH) \
	taskset --cpu-list 0 numactl -m0 \
	$(ELF_DIR)/tcp-ping-pong.elf --client 198.19.201.34:10000

prometheus-ping-pong-p41:
	sudo -E \
	CONFIG_PATH=$(CONFIG_DIR)/p41_config.yaml \
	$(ENV) \
	MIG_OFF=0 \
	LD_LIBRARY_PATH=$(LD_LIBRARY_PATH) \
	taskset --cpu-list 0 numactl -m0 \
	$(ELF_DIR)/tcp-ping-pong.elf --server 198.19.200.41:10000

prometheus-ping-pong-p42:
	sudo -E \
	CONFIG_PATH=$(CONFIG_DIR)/p42_config.yaml \
	$(ENV) \
	MIG_OFF=1 \
	LD_LIBRARY_PATH=$(LD_LIBRARY_PATH) \
	taskset --cpu-list 0 numactl -m0 \
	$(ELF_DIR)/tcp-ping-pong.elf --server 198.19.200.42:10000


redis-server-p41:
	cd ../capybara-redis/src && \
	sudo -E \
	$(ENV) \
	CONFIG_PATH=$(CONFIG_DIR)/p41_config.yaml \
	LD_LIBRARY_PATH=$(LD_LIBRARY_PATH) \
	LD_PRELOAD=$(LIBDIR)/libshim.so \
	./redis-server ../config/p41.conf

redis-server-p42:
	cd ../capybara-redis/src && \
	sudo -E \
	MIG_OFF=1 \
	$(ENV) \
	CONFIG_PATH=$(CONFIG_DIR)/p42_config.yaml \
	LD_LIBRARY_PATH=$(LD_LIBRARY_PATH) \
	LD_PRELOAD=$(LIBDIR)/libshim.so \
	./redis-server ../config/p42.conf


run-proxy-p41:
	sudo -E \
	CONFIG_PATH=$(CONFIG_DIR)/p41_config.yaml \
	$(ENV) \
	LD_LIBRARY_PATH=$(LD_LIBRARY_PATH) \
	taskset --cpu-list 2 numactl -m0 \
	$(ELF_DIR)/proxy.elf 198.19.200.41:10000 198.19.200.41:10001

run-proxy-p42:
	sudo -E \
	CONFIG_PATH=$(CONFIG_DIR)/p42_config.yaml \
	$(ENV) \
	MIG_OFF=1 \
	LD_LIBRARY_PATH=$(LD_LIBRARY_PATH) \
	taskset --cpu-list 2 numactl -m0 \
	$(ELF_DIR)/proxy.elf 198.19.200.42:10000 198.19.200.42:10001

run-proxy-node8:
	sudo -E \
	CONFIG_PATH=$(CONFIG_DIR)/node8_config.yaml \
	$(ENV) \
	MIG_OFF=1 \
	LD_LIBRARY_PATH=$(LD_LIBRARY_PATH) \
	taskset --cpu-list 2 numactl -m0 \
	$(ELF_DIR)/proxy.elf 10.0.1.8:10000 10.0.1.8:10001

run-proxy-node9:
	sudo -E \
	CONFIG_PATH=$(CONFIG_DIR)/node9_config.yaml \
	$(ENV) \
	MIG_OFF=1 \
	LD_LIBRARY_PATH=$(LD_LIBRARY_PATH) \
	taskset --cpu-list 2 numactl -m0 \
	$(ELF_DIR)/proxy.elf 10.0.1.9:10000 10.0.1.9:10001