# Copyright (c) Microsoft Corporation.
# Licensed under the MIT license.

# Suffix for executable files.
export EXEC_SUFFIX := elf

all: all-examples
	mkdir -p $(BINDIR)/examples/rust
	cp -f $(BUILD_DIR)/examples/tcp-echo-multiflow $(BINDIR)/examples/rust/tcp-echo-multiflow.$(EXEC_SUFFIX)

all-examples:
	@echo "$(CARGO) build --examples $(CARGO_FEATURES) $(CARGO_FLAGS)"
	$(CARGO) build --examples $(CARGO_FEATURES) $(CARGO_FLAGS)

clean:
	@rm -rf $(BINDIR)/examples/rust/tcp-echo-multiflow.$(EXEC_SUFFIX)