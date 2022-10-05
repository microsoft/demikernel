# Copyright (c) Microsoft Corporation.
# Licensed under the MIT license.

#=======================================================================================================================
# Toolchain Configuration
#=======================================================================================================================

# C
export CC := gcc
export CFLAGS := -Werror -Wall -Wextra -O3 -I $(INCDIR) -std=c99

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
export COMPILE_CMD = $(CC) $(CFLAGS) $@.o common.o -o $(BINDIR)/examples/c/$@.$(EXEC_SUFFIX) $(LIBS)

#=======================================================================================================================

# Builds everything.
all: common.o pipe-ping-pong udp-push-pop udp-ping-pong tcp-push-pop tcp-ping-pong

make-dirs:
	mkdir -p $(BINDIR)/examples/c

# Builds pipe ping pong test.
pipe-ping-pong: make-dirs pipe-ping-pong.o
	$(COMPILE_CMD)

# Builds UDP push pop test.
udp-push-pop: make-dirs common.o udp-push-pop.o
	$(COMPILE_CMD)

# Builds UDP ping pong test.
udp-ping-pong: make-dirs common.o udp-ping-pong.o
	$(COMPILE_CMD)

# Builds TCP push pop test.
tcp-push-pop: make-dirs common.o tcp-push-pop.o
	$(COMPILE_CMD)

# Builds TCP ping pong test.
tcp-ping-pong: make-dirs common.o tcp-ping-pong.o
	$(COMPILE_CMD)

# Cleans up all build artifacts.
clean:
	@rm -rf $(OBJ)
	@rm -rf $(BINDIR)/examples/c/pipe-ping-pong.$(EXEC_SUFFIX)
	@rm -rf $(BINDIR)/examples/c/udp-push-pop.$(EXEC_SUFFIX)
	@rm -rf $(BINDIR)/examples/c/udp-ping-pong.$(EXEC_SUFFIX)
	@rm -rf $(BINDIR)/examples/c/tcp-push-pop.$(EXEC_SUFFIX)
	@rm -rf $(BINDIR)/examples/c/tcp-ping-pong.$(EXEC_SUFFIX)

# Builds a C source file.
%.o: %.c
	$(CC) $(CFLAGS) $< -c -o $@
