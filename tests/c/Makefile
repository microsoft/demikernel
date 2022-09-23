# Copyright (c) Microsoft Corporation.
# Licensed under the MIT license.

# This is a trick to enable portability across MAKE and NMAKE.
# MAKE recognizes line continuation in comments but NMAKE doesn't.
# NMAKE               \
!ifndef 0 #           \
!include windows.mk # \
!else
include linux.mk
#                     \
!endif

