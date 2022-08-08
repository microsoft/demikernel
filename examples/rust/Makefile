# Copyright (c) Microsoft Corporation.
# Licensed under the MIT license.

# Suffix for executable files.
export EXEC_SUFFIX := elf

all: all-examples
	mkdir -p $(BINDIR)/examples/rust
	cp -f $(BUILD_DIR)/examples/udp-push-pop  $(BINDIR)/examples/rust/udp-push-pop.$(EXEC_SUFFIX)
	cp -f $(BUILD_DIR)/examples/udp-ping-pong $(BINDIR)/examples/rust/udp-ping-pong.$(EXEC_SUFFIX)
	cp -f $(BUILD_DIR)/examples/tcp-push-pop  $(BINDIR)/examples/rust/tcp-push-pop.$(EXEC_SUFFIX)
	cp -f $(BUILD_DIR)/examples/tcp-ping-pong $(BINDIR)/examples/rust/tcp-ping-pong.$(EXEC_SUFFIX)

all-examples:
	@echo "$(CARGO) build --examples $(CARGO_FEATURES) $(CARGO_FLAGS)"
	$(CARGO) build --examples $(CARGO_FEATURES) $(CARGO_FLAGS)

clean:
	@rm -rf $(BINDIR)/examples/rust/udp-push-pop.$(EXEC_SUFFIX)
	@rm -rf $(BINDIR)/examples/rust/udp-ping-pong.$(EXEC_SUFFIX)
	@rm -rf $(BINDIR)/examples/rust/tcp-push-pop.$(EXEC_SUFFIX)
	@rm -rf $(BINDIR)/examples/rust/tcp-ping-pong.$(EXEC_SUFFIX)