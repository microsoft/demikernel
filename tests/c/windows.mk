# Copyright (c) Microsoft Corporation.
# Licensed under the MIT license.

CC = cl

all: sizes syscalls

sizes: make-dirs sizes.obj
	$(CC) sizes.obj $(LIBS) /Fe: $(BINDIR)\sizes.exe

syscalls: make-dirs syscalls.obj
	$(CC) syscalls.obj $(LIBS) /Fe: $(BINDIR)\syscalls.exe

clean:
	IF EXIST sizes.obj del /Q sizes.obj
	IF EXIST syscalls.obj del /Q syscalls.obj
	IF EXIST $(BINDIR)\syscalls.exe del /S /Q $(BINDIR)\syscalls.exe

sizes.obj:
	$(CC) /I $(INCDIR) tests\c\sizes.c /c

syscalls.obj:
	$(CC) /I $(INCDIR) tests\c\syscalls.c /c

make-dirs:
	IF NOT EXIST $(BINDIR) mkdir $(BINDIR)
