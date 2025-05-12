# Copyright (c) Microsoft Corporation.
# Licensed under the MIT license.

#=======================================================================================================================
# Default Paths
#=======================================================================================================================

!ifndef PREFIX
PREFIX = $(USERPROFILE)
!endif

!ifndef INSTALL_PREFIX
INSTALL_PREFIX = $(USERPROFILE)
!endif

!ifndef LD_LIBRARY_PATH
LD_LIBRARY_PATH = $(USERPROFILE)/lib
!endif

!ifndef XDP_PATH
XDP_PATH = $(USERPROFILE)\xdp
!endif

#=======================================================================================================================
# Build Configuration
#=======================================================================================================================

BUILD = release
!if "$(DEBUG)" == "yes"
BUILD = dev
!ifndef RUST_LOG
RUST_LOG = trace
!endif
!endif

#=======================================================================================================================
# Project Directories
#=======================================================================================================================

!ifndef BINDIR
BINDIR = $(MAKEDIR)\bin
!endif

!ifndef INCDIR
INCDIR = $(MAKEDIR)\include
!endif

SRCDIR = $(MAKEDIR)\src

BUILD_DIR = $(MAKEDIR)\target\release
!if "$(BUILD)" == "dev"
BUILD_DIR = $(MAKEDIR)\target\debug
!endif

!ifndef INPUT
INPUT = $(MAKEDIR)\network_simulator\input
!endif

#=======================================================================================================================
# Toolchain Configuration
#=======================================================================================================================

# Rust
!ifndef CARGO
CARGO = $(USERPROFILE)\.cargo\bin\cargo.exe
!endif
CARGO_FLAGS = $(CARGO_FLAGS) --profile $(BUILD)

!ifndef RUSTFLAGS
RUSTFLAGS = -D warnings
!endif

#=======================================================================================================================
# Libraries
#=======================================================================================================================

DEMIKERNEL_LIB = $(BUILD_DIR)\demikernel.dll.lib
LIBS = $(DEMIKERNEL_LIB)
DEMIKERNEL_DLL = $(BUILD_DIR)\demikernel.dll

RECURSIVE_COPY_FORCE_NO_PROMPT = xcopy /S /E /Y /I

#=======================================================================================================================
# Build Parameters
#=======================================================================================================================

!ifndef LIBOS
LIBOS = catnap
!endif
CARGO_FEATURES = $(CARGO_FEATURES) --features=$(LIBOS)-libos

# Switch for DPDK
!if "$(LIBOS)" == "catnip"
!ifndef DRIVER
DRIVER = mlx5	# defaults to mlx5, set the DRIVER env var if you want to change this
!endif
CARGO_FEATURES = $(CARGO_FEATURES) --features=$(DRIVER)
!endif

# Switch for XDP
!if "$(LIBOS)" == "catpowder"
CARGO_FEATURES = $(CARGO_FEATURES) --features=libxdp
!endif

!if "$(PROFILER)" == "yes"
CARGO_FEATURES = $(CARGO_FEATURES) --features=profiler
!endif

!ifndef DIRECT_MAPPING
DIRECT_MAPPING = "not-set"
!endif

!if $(DIRECT_MAPPING) == "yes"
CARGO_FEATURES = $(CARGO_FEATURES) --features=direct-mapping
!else
!if $(DIRECT_MAPPING) != "no"
!if "$(DEBUG)" == "no"
CARGO_FEATURES = $(CARGO_FEATURES) --features=direct-mapping
!endif
!endif
!endif

CARGO_FEATURES = $(CARGO_FEATURES) $(FEATURES)

#=======================================================================================================================
# Targets
#=======================================================================================================================

all: init all-libs all-tests all-examples

init:
	git config --local core.hooksPath .githooks

doc:
	$(CARGO) doc $(FLAGS) --no-deps

# Copies demikernel artifacts to a INSTALL_PREFIX directory.
install:
	IF NOT EXIST $(INSTALL_PREFIX)\include mkdir $(INSTALL_PREFIX)\include
	IF NOT EXIST $(INSTALL_PREFIX)\lib mkdir $(INSTALL_PREFIX)\lib
	$(RECURSIVE_COPY_FORCE_NO_PROMPT) $(INCDIR) $(INSTALL_PREFIX)\include
	$(RECURSIVE_COPY_FORCE_NO_PROMPT) $(DEMIKERNEL_DLL) $(INSTALL_PREFIX)\lib
	$(RECURSIVE_COPY_FORCE_NO_PROMPT) $(DEMIKERNEL_LIB) $(INSTALL_PREFIX)\lib

#=======================================================================================================================
# Libs
#=======================================================================================================================

all-libs:
	@echo "LD_LIBRARY_PATH: $(LD_LIBRARY_PATH)"
	@echo "XDP_PATH: $(XDP_PATH)"
	set XDP_PATH=$(XDP_PATH)
	set RUSTFLAGS=$(RUSTFLAGS)
	$(CARGO) build --lib $(CARGO_FEATURES) $(CARGO_FLAGS)
	IF NOT EXIST $(BINDIR) mkdir $(BINDIR)
	$(RECURSIVE_COPY_FORCE_NO_PROMPT) $(DEMIKERNEL_DLL) $(BINDIR)
	$(RECURSIVE_COPY_FORCE_NO_PROMPT) $(DEMIKERNEL_LIB) $(BINDIR)

#=======================================================================================================================
# Tests
#=======================================================================================================================

all-tests: all-tests-rust all-tests-c

all-tests-rust: all-libs
	set RUSTFLAGS=$(RUSTFLAGS)
	$(CARGO) build --tests $(CARGO_FEATURES) $(CARGO_FLAGS)

all-tests-c: all-libs
	set BINDIR=$(BINDIR)
	set INCDIR=$(INCDIR)
	set LIBS=$(LIBS)
	$(MAKE) /C /F tests/windows.mk all

clean-tests: clean-tests-c

clean-tests-c:
	set BINDIR=$(BINDIR)
	$(MAKE) /C /F tests/windows.mk clean

#=======================================================================================================================
# Examples
#=======================================================================================================================

all-examples: all-examples-c all-examples-rust

all-examples-c: all-libs
	set BINDIR=$(BINDIR)
	set INCDIR=$(INCDIR)
	set LIBS=$(LIBS)
	$(MAKE) /C /F examples/c/windows.mk all

all-examples-rust: all-libs
	set BINDIR=$(BINDIR)
	set BUILD_DIR=$(BUILD_DIR)
	set CARGO_FEATURES=$(CARGO_FEATURES)
	set RUSTFLAGS=$(RUSTFLAGS)
	set CARGO_FLAGS=$(CARGO_FLAGS)
	set CARGO=$(CARGO)
	set INCDIR=$(INCDIR)
	$(MAKE) /C /F examples/rust/windows.mk all

clean-examples: clean-examples-c clean-examples-rust

clean-examples-c:
	set BINDIR=$(BINDIR)
	$(MAKE) /C /F examples/c/windows.mk clean

clean-examples-rust:
	set BINDIR=$(BINDIR)
	$(MAKE) /C /F examples/rust/windows.mk clean

#=======================================================================================================================
# Benchmarks
#=======================================================================================================================

bench:
	$(CARGO) bench $(CARGO_FEATURES) $(CARGO_FLAGS) -- --nocapture

#=======================================================================================================================
# Code formatting
#=======================================================================================================================

check-fmt: check-fmt-rust

check-fmt-rust:
	set RUSTFLAGS=$(RUSTFLAGS)
	$(CARGO) fmt --all -- --check

#=======================================================================================================================
# Clean
#=======================================================================================================================

clean: clean-examples clean-tests
	del /S /Q target
	del /Q Cargo.lock
	$(CARGO) clean

#=======================================================================================================================
# Tests
#=======================================================================================================================

!ifndef CONFIG_PATH
CONFIG_PATH = $(USERPROFILE)/config.yaml
!endif

!ifndef MTU
MTU = 1500
!endif

!ifndef PEER
PEER = server
!endif

!ifndef TEST
TEST = udp-push-pop
!endif

!ifndef TEST_INTEGRATION
TEST_INTEGRATION = tcp-tests
!endif

test-system: test-system-rust

test-system-rust:
	set RUST_LOG=$(RUST_LOG)
	set RUSTFLAGS=$(RUSTFLAGS)
	$(BINDIR)\examples\rust\$(TEST).exe $(ARGS)

test-unit: test-unit-rust

test-unit-c: all-tests-c test-unit-c-sizes test-unit-c-syscalls

test-unit-c-sizes: all-tests-c
	set RUST_LOG=$(RUST_LOG)
	set RUSTFLAGS=$(RUSTFLAGS)
	$(BINDIR)\sizes.exe

test-unit-c-syscalls: all-tests-c
	set RUST_LOG=$(RUST_LOG)
	set RUSTFLAGS=$(RUSTFLAGS)
	$(BINDIR)\syscalls.exe

test-unit-rust: test-unit-rust-lib test-unit-rust-udp test-unit-rust-tcp
	set RUST_LOG=$(RUST_LOG)
	set RUSTFLAGS=$(RUSTFLAGS)
	$(CARGO) test --test sga $(BUILD) $(CARGO_FEATURES) -- --nocapture --test-threads=1 test_unit_sga_alloc_free_single_small
	$(CARGO) test --test sga $(BUILD) $(CARGO_FEATURES) -- --nocapture --test-threads=1 test_unit_sga_alloc_free_loop_tight_small
	$(CARGO) test --test sga $(BUILD) $(CARGO_FEATURES) -- --nocapture --test-threads=1 test_unit_sga_alloc_free_loop_decoupled_small
	$(CARGO) test --test sga $(BUILD) $(CARGO_FEATURES) -- --nocapture --test-threads=1 test_unit_sga_alloc_free_single_big
	$(CARGO) test --test sga $(BUILD) $(CARGO_FEATURES) -- --nocapture --test-threads=1 test_unit_sga_alloc_free_loop_tight_big
	$(CARGO) test --test sga $(BUILD) $(CARGO_FEATURES) -- --nocapture --test-threads=1 test_unit_sga_alloc_free_loop_decoupled_big

test-unit-rust-lib: all-tests-rust
	set INPUT=$(INPUT)
	set RUST_LOG=$(RUST_LOG)
	set RUSTFLAGS=$(RUSTFLAGS)
	$(CARGO) test --lib $(CARGO_FLAGS) $(CARGO_FEATURES) -- --nocapture $(TEST_UNIT)

test-unit-rust-udp: all-tests-rust
	set RUST_LOG=$(RUST_LOG)
	set RUSTFLAGS=$(RUSTFLAGS)
	$(CARGO) test --test udp $(CARGO_FLAGS) $(CARGO_FEATURES) -- --nocapture $(TEST_UNIT)

test-unit-rust-tcp: all-tests-rust
	set RUST_LOG=$(RUST_LOG)
	set RUSTFLAGS=$(RUSTFLAGS)
	$(CARGO) test --test tcp $(CARGO_FLAGS) $(CARGO_FEATURES) -- --nocapture $(TEST_UNIT)

test-integration-rust:
	set RUST_LOG=$(RUST_LOG)
	set RUSTFLAGS=$(RUSTFLAGS)
	$(CARGO) test --test $(TEST_INTEGRATION) $(CARGO_FLAGS) $(CARGO_FEATURES) -- $(ARGS)
