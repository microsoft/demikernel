# Copyright (c) Microsoft Corporation.
# Licensed under the MIT license.

CC = cl

all: benchmarks

benchmarks: make-dirs benchmarks.obj
	$(CC) benchmarks.obj $(LIBS) /Fe: $(BINDIR)\benchmarks.exe

clean:
	IF EXIST benchmarks.obj del /Q benchmarks.obj
	IF EXIST $(BINDIR)\benchmarks.exe del /S /Q $(BINDIR)\benchmarks.exe

benchmarks.obj:
	$(CC) /I $(INCDIR) benchmarks\c\main.c /c

make-dirs:
	IF NOT EXIST $(BINDIR) mkdir $(BINDIR)
