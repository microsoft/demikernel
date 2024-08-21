# Copyright (c) Microsoft Corporation.
# Licensed under the MIT license.

# Suffix for executable files.
export EXEC_SUFFIX := elf

all: all-examples
	mkdir -p $(BINDIR)/examples/rust
#	cp -f $(BUILD_DIR)/examples/tcp-ping-pong $(BINDIR)/examples/rust/tcp-ping-pong.$(EXEC_SUFFIX)
	cp -f $(BUILD_DIR)/examples/proxy $(BINDIR)/examples/rust/proxy.$(EXEC_SUFFIX)

export EXAMPLE_FEATURES ?= --features=tcp-migration
# export EXAMPLE_FEATURES ?= --features=capy-log,profiler
all-examples:
#	@echo "$(CARGO) build --examples $(CARGO_FEATURES) $(CARGO_FLAGS)"
#	$(CARGO) build --examples $(CARGO_FEATURES) $(CARGO_FLAGS)
#	@echo "$(CARGO) build --example tcp-ping-pong $(CARGO_FEATURES) $(CARGO_FLAGS) $(EXAMPLE_FEATURES)"
#	$(CARGO) build --example tcp-ping-pong $(CARGO_FEATURES) $(CARGO_FLAGS) $(EXAMPLE_FEATURES)
	@echo "$(CARGO) build --example proxy $(CARGO_FEATURES) $(CARGO_FLAGS) $(EXAMPLE_FEATURES)"
	$(CARGO) build --example proxy $(CARGO_FEATURES) $(CARGO_FLAGS) $(EXAMPLE_FEATURES)

clean:
	@rm -rf $(BINDIR)/examples/rust/udp-dump.$(EXEC_SUFFIX)
	@rm -rf $(BINDIR)/examples/rust/udp-echo.$(EXEC_SUFFIX)
	@rm -rf $(BINDIR)/examples/rust/udp-pktgen.$(EXEC_SUFFIX)
	@rm -rf $(BINDIR)/examples/rust/udp-relay.$(EXEC_SUFFIX)
	@rm -rf $(BINDIR)/examples/rust/pipe-ping-pong.$(EXEC_SUFFIX)
	@rm -rf $(BINDIR)/examples/rust/pipe-push-pop.$(EXEC_SUFFIX)
	@rm -rf $(BINDIR)/examples/rust/udp-push-pop.$(EXEC_SUFFIX)
	@rm -rf $(BINDIR)/examples/rust/udp-ping-pong.$(EXEC_SUFFIX)
	@rm -rf $(BINDIR)/examples/rust/tcp-dump.$(EXEC_SUFFIX)
	@rm -rf $(BINDIR)/examples/rust/tcp-echo.$(EXEC_SUFFIX)
	@rm -rf $(BINDIR)/examples/rust/tcp-pktgen.$(EXEC_SUFFIX)
	@rm -rf $(BINDIR)/examples/rust/tcp-push-pop.$(EXEC_SUFFIX)
	@rm -rf $(BINDIR)/examples/rust/tcp-ping-pong.$(EXEC_SUFFIX)
	@rm -rf $(BINDIR)/examples/rust/tcp-close.$(EXEC_SUFFIX)
	@rm -rf $(BINDIR)/examples/rust/pipe-open.$(EXEC_SUFFIX)
	@rm -rf $(BINDIR)/examples/rust/tcp-wait.$(EXEC_SUFFIX)
