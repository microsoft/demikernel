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

#=======================================================================================================================
# Build Configuration
#=======================================================================================================================

BUILD = release
!if "$(DEBUG)" == "yes"
BUILD = dev
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

#=======================================================================================================================
# Toolchain Configuration
#=======================================================================================================================

# Rust
!ifndef CARGO
CARGO = $(USERPROFILE)\.cargo\bin\cargo.exe
!endif
CARGO_FLAGS = $(CARGO_FLAGS) --profile $(BUILD)

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
LIBOS = catnapw
!endif
CARGO_FEATURES = $(CARGO_FEATURES) --features=$(LIBOS)-libos

# Switch for DPDK
!if "$(LIBOS)" == "catnip"
!ifndef DRIVER
DRIVER = mlx5	# defaults to mlx5, set the DRIVER env var if you want to change this
!endif
CARGO_FEATURES = $(CARGO_FEATURES) --features=$(DRIVER)
!endif

# Switch for profiler.
!if "$(PROFILER)" == "yes"
CARGO_FEATURES = $(CARGO_FEATURES) --features=profiler
!endif

CARGO_FEATURES = $(CARGO_FEATURES) $(FEATURES)

#=======================================================================================================================

all: all-libs all-tests all-examples

# Builds documentation.
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

# Builds all libraries.
all-libs:
	@echo "LD_LIBRARY_PATH: $(LD_LIBRARY_PATH)"
	@echo "$(CARGO) build --libs $(CARGO_FEATURES) $(CARGO_FLAGS)"
	$(CARGO) build --lib $(CARGO_FEATURES) $(CARGO_FLAGS)
	$(RECURSIVE_COPY_FORCE_NO_PROMPT) $(DEMIKERNEL_DLL) $(BINDIR)
	$(RECURSIVE_COPY_FORCE_NO_PROMPT) $(DEMIKERNEL_LIB) $(BINDIR)

#=======================================================================================================================
# Tests
#=======================================================================================================================

# Builds all tests.
all-tests: all-tests-rust all-tests-c

# Builds all Rust tests.
all-tests-rust: all-libs
	@echo "$(CARGO) build --tests $(CARGO_FEATURES) $(CARGO_FLAGS)"
	$(CARGO) build --tests $(CARGO_FEATURES) $(CARGO_FLAGS)

# Builds all C tests.
all-tests-c: all-libs
	set BINDIR=$(BINDIR)
	set INCDIR=$(INCDIR)
	set LIBS=$(LIBS)
	$(MAKE) /C /F tests/windows.mk all

# Cleans up all build artifactos for tests.
clean-tests: clean-tests-c

# Cleans up all C build artifacts for tests.
clean-tests-c:
	set BINDIR=$(BINDIR)
	$(MAKE) /C /F tests/windows.mk clean

#=======================================================================================================================
# Examples
#=======================================================================================================================

# Builds all examples.
all-examples: all-examples-c all-examples-rust

# Builds all C examples.
all-examples-c: all-libs
	set BINDIR=$(BINDIR)
	set INCDIR=$(INCDIR)
	set LIBS=$(LIBS)
	$(MAKE) /C /F examples/c/windows.mk all

# Builds all Rust examples.
all-examples-rust: all-libs
	set BINDIR=$(BINDIR)
	set BUILD_DIR=$(BUILD_DIR)
	set CARGO_FEATURES=$(CARGO_FEATURES)
	set CARGO_FLAGS=$(CARGO_FLAGS)
	set CARGO=$(CARGO)
	set INCDIR=$(INCDIR)
	$(MAKE) /C /F examples/rust/windows.mk all

# Cleans all examples.
clean-examples: clean-examples-c clean-examples-rust

# Cleans all C examples.
clean-examples-c:
	set BINDIR=$(BINDIR)
	$(MAKE) /C /F examples/c/windows.mk clean

# Cleans all Rust examples.
clean-examples-rust:
	set BINDIR=$(BINDIR)
	$(MAKE) /C /F examples/rust/windows.mk clean

#=======================================================================================================================
# Check
#=======================================================================================================================

# Check code style formatting.
check-fmt: check-fmt-rust

# Check code style formatting for Rust.
check-fmt-rust:
	$(CARGO) fmt --all -- --check

#=======================================================================================================================
# Clean
#=======================================================================================================================

# Cleans up all build artifacts.
clean: clean-examples clean-tests
	del /S /Q target
	del /Q Cargo.lock
	$(CARGO) clean

#=======================================================================================================================
# Tests section
#=======================================================================================================================

!ifndef CONFIG_PATH
CONFIG_PATH = $(USERPROFILE)/config.yaml
!endif

!ifndef MTU
MTU = 1500
!endif

!ifndef MSS
MSS = 1500
!endif

!ifndef PEER
PEER = server
!endif

!ifndef TEST
TEST = udp-push-pop
!endif

# Runs system tests.
test-system: test-system-rust

# Rust system tests.
test-system-rust:
	$(BINDIR)/examples/rust/$(TEST).exe $(ARGS)

# Runs unit tests.
test-unit: test-unit-rust

# C unit tests.
test-unit-c: all-tests-c
	$(BINDIR)\syscalls.exe

# Rust unit tests.
test-unit-rust:
	$(CARGO) test --lib $(CARGO_FLAGS) $(CARGO_FEATURES) -- --nocapture
	$(CARGO) test --test udp $(CARGO_FLAGS) $(CARGO_FEATURES) -- --nocapture
	$(CARGO) test --test tcp $(CARGO_FLAGS) $(CARGO_FEATURES) -- --nocapture
	$(CARGO) test --test sga $(BUILD) $(CARGO_FEATURES) -- --nocapture --test-threads=1 test_unit_sga_alloc_free_single_small
	$(CARGO) test --test sga $(BUILD) $(CARGO_FEATURES) -- --nocapture --test-threads=1 test_unit_sga_alloc_free_loop_tight_small
	$(CARGO) test --test sga $(BUILD) $(CARGO_FEATURES) -- --nocapture --test-threads=1 test_unit_sga_alloc_free_loop_decoupled_small
	$(CARGO) test --test sga $(BUILD) $(CARGO_FEATURES) -- --nocapture --test-threads=1 test_unit_sga_alloc_free_single_big
	$(CARGO) test --test sga $(BUILD) $(CARGO_FEATURES) -- --nocapture --test-threads=1 test_unit_sga_alloc_free_loop_tight_big
	$(CARGO) test --test sga $(BUILD) $(CARGO_FEATURES) -- --nocapture --test-threads=1 test_unit_sga_alloc_free_loop_decoupled_big
