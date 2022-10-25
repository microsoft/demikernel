# Copyright (c) Microsoft Corporation.
# Licensed under the MIT license.

CC = cl

all: syscalls

syscalls: make-dirs syscalls.obj
	$(CC) syscalls.obj $(LIBS) /Fe: $(BINDIR)\syscalls.exe

clean:
	IF EXIST syscalls.obj del /Q syscalls.obj
	IF EXIST $(BINDIR)\syscalls.exe del /S /Q $(BINDIR)\syscalls.exe

syscalls.obj:
	$(CC) /I $(INCDIR) tests\c\syscalls.c /c

make-dirs:
	IF NOT EXIST $(BINDIR) mkdir $(BINDIR)
