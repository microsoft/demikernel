# Copyright (c) Microsoft Corporation.
# Licensed under the MIT license.

#=======================================================================================================================
# Project Directories
#=======================================================================================================================

export INCDIR := $(CURDIR)/include
export SRCDIR := $(CURDIR)/src

#=======================================================================================================================
# Toolchain Configuration
#=======================================================================================================================

# Compiler Flags
export CFLAGS += -I $(INCDIR)

#=======================================================================================================================
# Libraries
#=======================================================================================================================

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
