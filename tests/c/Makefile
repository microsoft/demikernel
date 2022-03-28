# Copyright (c) Microsoft Corporation.
# Licensed under the MIT license.

#===============================================================================
# Toolchain Configuration
#===============================================================================

export CC := gcc
export CFLAGS := -Wall -Wextra -O3 -I $(INCDIR) -std=c99
#===============================================================================

all: all-syscalls

all-syscalls:
	$(CC) $(CFLAGS) syscalls.c -o $(BINDIR)/syscalls $(LIBS)

clean: clean-syscalls

clean-syscalls:
	@rm -rf $(BINDIR)/syscalls