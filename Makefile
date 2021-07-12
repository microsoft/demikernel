# Copyright (c) Microsoft Corporation.
# Licensed under the MIT license.

export PREFIX ?= $(HOME)

export PKG_CONFIG_PATH ?= $(shell find $(PREFIX)/lib -name '*pkgconfig*' -type d)

export SRCDIR = $(CURDIR)/src
export BINDIR = $(CURDIR)/bin
export LIBDIR = $(CURDIR)/lib
export CONTRIBDIR = $(CURDIR)/submodules
export BUILDDIR = $(CURDIR)/build

#===============================================================================

export CARGO_FLAGS ?= "--release"

#===============================================================================

all: demikernel

clean: demikernel-clean

demikernel:
	cd $(SRCDIR) && \
	cargo build $(CARGO_FLAGS)

demikernel-examples:
	cd $(SRCDIR) && \
	cargo build --examples $(CARGO_FLAGS)

demikernel-clean:
	cd $(SRCDIR) && \
	cargo clean

dpdk:
	cd $(CONTRIBDIR)/dpdk && \
	mkdir -p $(BUILDDIR)/dpdk && \
	meson --prefix=$(PREFIX) $(BUILDDIR)/dpdk && \
	cd $(BUILDDIR)/dpdk && \
	ninja && \
	ninja install && \
	rm -rf $(BUILDDIR)/dpdk
