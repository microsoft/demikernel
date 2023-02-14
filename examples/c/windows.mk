# Copyright (c) Microsoft Corporation.
# Licensed under the MIT license.

# C/C++ compiler
CC = cl

LIBS = $(LIBS) "WS2_32.lib"

!if "$(DEBUG)" == "yes"
CFLAGS = $(CFLAGS) /Zi
!endif

# Compiles several object files into a binary.
COMPILE_CMD = $(CC) $(CFLAGS) $@.obj common.obj $(LIBS) /Fe: $(BINDIR)/examples/c/$@.exe

# Builds everything.
all: udp-push-pop udp-ping-pong tcp-push-pop tcp-ping-pong

make-dirs:
	IF NOT EXIST $(BINDIR)\examples\c mkdir $(BINDIR)\examples\c

# Builds UDP push pop test.
udp-push-pop: make-dirs udp-push-pop.obj
	$(COMPILE_CMD)

# Builds UDP ping pong test.
udp-ping-pong: make-dirs udp-ping-pong.obj
	$(COMPILE_CMD)

# Builds TCP push pop test.
tcp-push-pop: make-dirs tcp-push-pop.obj
	$(COMPILE_CMD)

# Builds TCP ping pong test.
tcp-ping-pong: make-dirs tcp-ping-pong.obj
	$(COMPILE_CMD)

# Cleans up all build artifacts.
clean:
	del /S /Q *.obj
	IF EXIST $(BINDIR)\examples\c\udp-push-pop.exe del /Q $(BINDIR)\examples\c\udp-push-pop.exe
	IF EXIST $(BINDIR)\examples\c\udp-ping-pong.exe del /Q $(BINDIR)\examples\c\udp-ping-pong.exe
	IF EXIST $(BINDIR)\examples\c\tcp-push-pop.exe del /Q $(BINDIR)\examples\c\tcp-push-pop.exe
	IF EXIST $(BINDIR)\examples\c\tcp-ping-pong.exe del /Q $(BINDIR)\examples\c\tcp-ping-pong.exe

# Build obj files.
common.obj:
	$(CC) /I $(INCDIR) /I examples\c\ examples\c\common.c /c

udp-push-pop.obj: common.obj
	$(CC) /I $(INCDIR) /I examples\c\ examples\c\udp-push-pop.c /c

udp-ping-pong.obj: common.obj
	$(CC) /I $(INCDIR) /I examples\c\ examples\c\udp-ping-pong.c /c

tcp-push-pop.obj: common.obj
	$(CC) /I $(INCDIR) /I examples\c\ examples\c\tcp-push-pop.c /c

tcp-ping-pong.obj: common.obj
	$(CC) /I $(INCDIR) /I examples\c\ examples\c\tcp-ping-pong.c /c
