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

export DRIVER ?= $(shell [ ! -z "`lspci | grep -E "ConnectX-[4,5]"`" ] && echo mlx5 || echo mlx4)
export BUILD ?= --release

#===============================================================================

all: demikernel-all demikernel-tests

clean: demikernel-clean

demikernel-all:
	cd $(SRCDIR) && \
	$(CARGO) build --all $(BUILD) --features=$(DRIVER) $(CARGO_FLAGS)

demikernel-tests:
	cd $(SRCDIR) && \
	$(CARGO) build --tests $(BUILD) --features=$(DRIVER) $(CARGO_FLAGS)

demikernel-clean:
	cd $(SRCDIR) &&   \
	rm -rf target &&  \
	$(CARGO) clean && \
	rm -f Cargo.lock

test:
	cd $(SRCDIR) && \
	sudo -E LD_LIBRARY_PATH="$(LD_LIBRARY_PATH)" timeout $(TIMEOUT) $(CARGO) test $(BUILD) --features=$(DRIVER) $(CARGO_FLAGS) -- --nocapture $(TEST)
