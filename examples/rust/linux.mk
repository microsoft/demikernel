# Copyright (c) Microsoft Corporation.
# Licensed under the MIT license.

# Suffix for executable files.
export EXEC_SUFFIX := elf

all: all-examples
	mkdir -p $(BINDIR)/examples/rust
	cp -f $(BUILD_DIR)/examples/udp-dump  $(BINDIR)/examples/rust/udp-dump.$(EXEC_SUFFIX)
	cp -f $(BUILD_DIR)/examples/udp-echo  $(BINDIR)/examples/rust/udp-echo.$(EXEC_SUFFIX)
	cp -f $(BUILD_DIR)/examples/udp-pktgen  $(BINDIR)/examples/rust/udp-pktgen.$(EXEC_SUFFIX)
	cp -f $(BUILD_DIR)/examples/udp-relay  $(BINDIR)/examples/rust/udp-relay.$(EXEC_SUFFIX)
	cp -f $(BUILD_DIR)/examples/pipe-ping-pong  $(BINDIR)/examples/rust/pipe-ping-pong.$(EXEC_SUFFIX)
	cp -f $(BUILD_DIR)/examples/pipe-push-pop  $(BINDIR)/examples/rust/pipe-push-pop.$(EXEC_SUFFIX)
	cp -f $(BUILD_DIR)/examples/udp-push-pop  $(BINDIR)/examples/rust/udp-push-pop.$(EXEC_SUFFIX)
	cp -f $(BUILD_DIR)/examples/udp-ping-pong $(BINDIR)/examples/rust/udp-ping-pong.$(EXEC_SUFFIX)
	cp -f $(BUILD_DIR)/examples/tcp-dump  $(BINDIR)/examples/rust/tcp-dump.$(EXEC_SUFFIX)
	cp -f $(BUILD_DIR)/examples/tcp-echo  $(BINDIR)/examples/rust/tcp-echo.$(EXEC_SUFFIX)
	cp -f $(BUILD_DIR)/examples/tcp-pktgen  $(BINDIR)/examples/rust/tcp-pktgen.$(EXEC_SUFFIX)
	cp -f $(BUILD_DIR)/examples/tcp-push-pop  $(BINDIR)/examples/rust/tcp-push-pop.$(EXEC_SUFFIX)
	cp -f $(BUILD_DIR)/examples/tcp-ping-pong $(BINDIR)/examples/rust/tcp-ping-pong.$(EXEC_SUFFIX)
	cp -f $(BUILD_DIR)/examples/tcp-close $(BINDIR)/examples/rust/tcp-close.$(EXEC_SUFFIX)
	cp -f $(BUILD_DIR)/examples/pipe-open $(BINDIR)/examples/rust/pipe-open.$(EXEC_SUFFIX)
	cp -f $(BUILD_DIR)/examples/tcp-wait $(BINDIR)/examples/rust/tcp-wait.$(EXEC_SUFFIX)

all-examples:
	@echo "$(CARGO) build --examples $(CARGO_FEATURES) $(CARGO_FLAGS)"
	$(CARGO) build --examples $(CARGO_FEATURES) $(CARGO_FLAGS)

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
