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
export CFLAGS := -Werror -Wall -Wextra -O3 -I $(INCDIR) -std=c11
export CFLAGS += -D_POSIX_C_SOURCE=199309L

#=======================================================================================================================
# Build Artifacts
#=======================================================================================================================

# C source files.
export SRC := $(wildcard *.c)

# Object files.
export OBJ := $(SRC:.c=.o)

# Suffix for executable files.
export EXEC_SUFFIX := elf

# Compiles several object files into a binary.
export COMPILE_CMD = $(CC) $(CFLAGS) $(OBJ) -o $(BINDIR)/$@.$(EXEC_SUFFIX) $(LIBS)

#=======================================================================================================================

# Builds everything.
all: benchmarks

make-dirs:
	mkdir -p $(BINDIR)

# Builds system call test.
benchmarks: make-dirs $(OBJ)
	$(COMPILE_CMD)

# Cleans up all build artifacts.
clean:
	@rm -rf $(OBJ)
	@rm -rf $(BINDIR)/benchmarks.$(EXEC_SUFFIX)

# Builds a C source file.
%.o: %.c
	$(CC) $(CFLAGS) $< -c -o $@
