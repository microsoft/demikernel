# Copyright (c) Microsoft Corporation.
# Licensed under the MIT license.

#===============================================================================
# Toolchain Configuration
#===============================================================================

export CC := gcc
export CFLAGS := -Wall -Wextra -O3 -I $(INCDIR) -std=c99

#===============================================================================

all: all-syscalls all-udp-push-pop all-tcp-push-pop

all-syscalls:
	$(CC) $(CFLAGS) syscalls.c -o $(BINDIR)/syscalls $(LIBS)

all-udp-push-pop:
	$(CC) $(CFLAGS) udp-push-pop.c -o $(BINDIR)/udp-push-pop $(LIBS)

all-tcp-push-pop:
	$(CC) $(CFLAGS) tcp-push-pop.c -o $(BINDIR)/tcp-push-pop $(LIBS)

clean: clean-syscalls

clean-syscalls:
	@rm -rf $(BINDIR)/syscalls
	@rm -rf $(BINDIR)/udp-push-pop
	@rm -rf $(BINDIR)/tcp-push-pop
