# Copyright (c) Microsoft Corporation.
# Licensed under the MIT license.

#=======================================================================================================================
# Default Paths
#=======================================================================================================================

export PREFIX ?= $(HOME)
export DEMIKERNEL_HOME ?= $(HOME)
export INSTALL_PREFIX ?= $(DEMIKERNEL_HOME)

#=======================================================================================================================
# Project Directories
#=======================================================================================================================

export BINDIR ?= $(CURDIR)/bin
export LIBDIR ?= $(CURDIR)/lib
export INCDIR ?= $(CURDIR)/include
export SRCDIR = $(CURDIR)/src

#=======================================================================================================================
# Toolchain Configuration
#=======================================================================================================================

# Compiler
export CC := gcc

# Compiler Flags
export CFLAGS := -std=c99 -D _POSIX_C_SOURCE=199309L
export CFLAGS += -Wall -Wextra -Werror
export CFLAGS += -I $(INCDIR) -I $(DEMIKERNEL_HOME)/include

#=======================================================================================================================
# Libraries
#=======================================================================================================================

# Demikernel Library
export DEMIKERNEL_LIB := $(DEMIKERNEL_HOME)/lib/libdemikernel.so

# SHIM Library for Demikernel
export SHIM_LIB := libshim.so

#=======================================================================================================================

# Builds all artifacts.
all:
	$(MAKE) -C $(SRCDIR) all

# Copies build artifacts to the INSTALL_PREFIX directory.
install: all
	mkdir -p $(INSTALL_PREFIX)/include $(INSTALL_PREFIX)/lib
	cp -f  $(LIBDIR)/$(SHIM_LIB) $(INSTALL_PREFIX)/lib/

# Check code style formatting.
check-fmt:
	$(shell find include/ -name "*.h" -name "*.hxx" -name "*.c" -name "*.cpp" -type f -print0 | xargs -0 clang-format --fallback-style=Microsoft --dry-run -Werror )
	@exit $(.SHELLSTATUS)

# Cleans up all build artifacts.
clean:
	$(MAKE) -C $(SRCDIR) clean
