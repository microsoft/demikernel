# Copyright (c) Microsoft Corporation.
# Licensed under the MIT license.

export PREFIX ?= $(HOME)

export PKG_CONFIG_PATH ?= $(shell find $(PREFIX)/lib/ -name '*pkgconfig*' -type d)
export LD_LIBRARY_PATH ?= $(shell find $(PREFIX)/lib/ -name '*x86_64-linux-gnu*' -type d)
export CONFIG_PATH ?= $(HOME)/config.yaml

export CARGO ?= $(HOME)/.cargo/bin/cargo
export TIMEOUT ?= 30

export SRCDIR = $(CURDIR)/src
export BINDIR = $(CURDIR)/bin
export LIBDIR = $(CURDIR)/lib
export CONTRIBDIR = $(CURDIR)/submodules
export BUILDDIR = $(CURDIR)/build

#===============================================================================

export DRIVER ?= mlx5
export BUILD ?= --release

export CARGO_FLAGS ?= $(BUILD) --features=$(DRIVER)

#===============================================================================

all: demikernel-all demikernel-tests

clean: demikernel-clean

demikernel-all:
	cd $(SRCDIR) && \
	$(CARGO) build --all $(CARGO_FLAGS)

demikernel-tests:
	cd $(SRCDIR) && \
	$(CARGO) build --tests $(CARGO_FLAGS)

demikernel-clean:
	cd $(SRCDIR) &&   \
	$(CARGO) clean && \
	rm -f Cargo.lock

test:
	cd $(SRCDIR) && \
	sudo -E LD_LIBRARY_PATH="$(LD_LIBRARY_PATH)" timeout $(TIMEOUT) $(CARGO) test $(CARGO_FLAGS) -- --nocapture $(TEST)

dpdk:
	cd $(CONTRIBDIR)/dpdk && \
	mkdir -p $(BUILDDIR)/dpdk && \
	meson --prefix=$(PREFIX) $(BUILDDIR)/dpdk && \
	cd $(BUILDDIR)/dpdk && \
	ninja && \
	ninja install && \
	rm -rf $(BUILDDIR)/dpdk
