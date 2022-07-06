# Copyright (c) Microsoft Corporation.
# Licensed under the MIT license.

#=======================================================================================================================
# Toolchain Configuration
#=======================================================================================================================

export CC := gcc
export CFLAGS := -Wall -Wextra -O3 -I $(INCDIR) -std=c99

#=======================================================================================================================
# Build Artifacts
#=======================================================================================================================

# C source files.
export SRC_C := $(wildcard *.c)

# Object files.
export OBJ := $(SRC_C:.c=.o)

# Suffix for executable files.
export EXEC_SUFFIX := elf

#=======================================================================================================================

# Builds everything.
all: $(OBJ) syscalls udp-push-pop tcp-push-pop

# Builds system call test.
syscalls: syscalls.o
	$(CC) $(CFLAGS) $< -o $(BINDIR)/$@.$(EXEC_SUFFIX) $(LIBS)

# Builds UDP push pop test.
udp-push-pop: udp-push-pop.o
	$(CC) $(CFLAGS) $< -o $(BINDIR)/$@.$(EXEC_SUFFIX) $(LIBS)

# Builds TCP push pop test.
tcp-push-pop: tcp-push-pop.o
	$(CC) $(CFLAGS) $< -o $(BINDIR)/$@.$(EXEC_SUFFIX) $(LIBS)

# Cleans up all build artifacts.
clean:
	@rm -rf $(OBJ)
	@rm -rf $(BINDIR)/syscalls.$(EXEC_SUFFIX)
	@rm -rf $(BINDIR)/udp-push-pop.$(EXEC_SUFFIX)
	@rm -rf $(BINDIR)/tcp-push-pop.$(EXEC_SUFFIX)

# Builds a C source file.
%.o: %.c
	$(CC) $(CFLAGS) $< -c -o $@
