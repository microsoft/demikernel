# Copyright (c) Microsoft Corporation.
# Licensed under the MIT license.

#=======================================================================================================================
# Toolchain Configuration
#=======================================================================================================================

# Rust
export CARGO ?= $(HOME)/.cargo/bin/cargo
export CARGO_FLAGS += --profile $(BUILD)

# C
export CC := gcc
export CFLAGS := -Werror -Wall -Wextra -O3 -I $(INCDIR) -std=c99
export CFLAGS += -D_POSIX_C_SOURCE=199309L

#=======================================================================================================================
# Build Artifacts
#=======================================================================================================================

# C source files.
export SRC_C := $(wildcard *.c)

# Object files.
export OBJ := $(SRC_C:.c=.o)

# Suffix for executable files.
export EXEC_SUFFIX := elf

# Compiles several object files into a binary.
export COMPILE_CMD = $(CC) $(CFLAGS) $@.o -o $(BINDIR)/$@.$(EXEC_SUFFIX) $(LIBS)

#=======================================================================================================================

# Builds everything.
all: syscalls

make-dirs:
	mkdir -p $(BINDIR)

# Builds system call test.
syscalls: make-dirs syscalls.o
	$(COMPILE_CMD)

# Cleans up all build artifacts.
clean:
	@rm -rf $(OBJ)
	@rm -rf $(BINDIR)/syscalls.$(EXEC_SUFFIX)

# Builds a C source file.
%.o: %.c
	$(CC) $(CFLAGS) $< -c -o $@
