# Copyright (c) Microsoft Corporation.
# Licensed under the MIT license.

all:
	set BINDIR = $(BINDIR)
	set INCDIR = $(INCDIR)
	$(MAKE) /C /F benchmarks/c/windows.mk all

clean:
	set BINDIR = $(BINDIR)
	$(MAKE) /C /F benchmarks/c/windows.mk clean
