# Copyright (c) Microsoft Corporation.
# Licensed under the MIT license.

all: all-examples
	IF NOT EXIST $(BINDIR)\examples\rust mkdir $(BINDIR)\examples\rust
	copy /Y $(BUILD_DIR)\examples\udp-dump.exe $(BINDIR)\examples\rust\udp-dump.exe
	copy /Y $(BUILD_DIR)\examples\udp-echo.exe $(BINDIR)\examples\rust\udp-echo.exe
	copy /Y $(BUILD_DIR)\examples\udp-pktgen.exe $(BINDIR)\examples\rust\udp-pktgen.exe
	copy /Y $(BUILD_DIR)\examples\udp-relay.exe $(BINDIR)\examples\rust\udp-relay.exe
	copy /Y $(BUILD_DIR)\examples\udp-push-pop.exe $(BINDIR)\examples\rust\udp-push-pop.exe
	copy /Y $(BUILD_DIR)\examples\udp-ping-pong.exe $(BINDIR)\examples\rust\udp-ping-pong.exe
	copy /Y $(BUILD_DIR)\examples\tcp-dump.exe $(BINDIR)\examples\rust\tcp-dump.exe
	copy /Y $(BUILD_DIR)\examples\tcp-echo.exe $(BINDIR)\examples\rust\tcp-echo.exe
	copy /Y $(BUILD_DIR)\examples\tcp-pktgen.exe $(BINDIR)\examples\rust\tcp-pktgen.exe
	copy /Y $(BUILD_DIR)\examples\tcp-push-pop.exe $(BINDIR)\examples\rust\tcp-push-pop.exe
	copy /Y $(BUILD_DIR)\examples\tcp-ping-pong.exe $(BINDIR)\examples\rust\tcp-ping-pong.exe

all-examples:
	@echo "$(CARGO) build --examples $(CARGO_FEATURES) $(CARGO_FLAGS)"
	$(CARGO) build --examples $(CARGO_FEATURES) $(CARGO_FLAGS)

clean:
	IF EXIST $(BINDIR)\examples\rust\udp-dump.exe del /S /Q $(BINDIR)\examples\rust\udp-dump.exe
	IF EXIST $(BINDIR)\examples\rust\udp-echo.exe del /S /Q $(BINDIR)\examples\rust\udp-echo.exe
	IF EXIST $(BINDIR)\examples\rust\udp-pktgen.exe del /S /Q $(BINDIR)\examples\rust\udp-pktgen.exe
	IF EXIST $(BINDIR)\examples\rust\udp-relay.exe del /S /Q $(BINDIR)\examples\rust\udp-relay.exe
	IF EXIST $(BINDIR)\examples\rust\udp-push-pop.exe del /S /Q $(BINDIR)\examples\rust\udp-push-pop.exe
	IF EXIST $(BINDIR)\examples\rust\udp-ping-pong.exe del /S /Q $(BINDIR)\examples\rust\udp-ping-pong.exe
	IF EXIST $(BINDIR)\examples\rust\tcp-dump.exe del /S /Q $(BINDIR)\examples\rust\tcp-dump.exe
	IF EXIST $(BINDIR)\examples\rust\tcp-echo.exe del /S /Q $(BINDIR)\examples\rust\tcp-echo.exe
	IF EXIST $(BINDIR)\examples\rust\tcp-pktgen.exe del /S /Q $(BINDIR)\examples\rust\tcp-pktgen.exe
	IF EXIST $(BINDIR)\examples\rust\tcp-push-pop.exe del /S /Q $(BINDIR)\examples\rust\tcp-push-pop.exe
	IF EXIST $(BINDIR)\examples\rust\tcp-ping-pong.exe del /S /Q $(BINDIR)\examples\rust\tcp-ping-pong.exe
