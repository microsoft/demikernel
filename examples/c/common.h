// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

/* This should come first. */
#define _POSIX_C_SOURCE 200809L

#ifndef COMMON_H_IS_INCLUDED
#define COMMON_H_IS_INCLUDED

#include <signal.h>
#include <stdio.h>
#include <stdlib.h>

#ifdef __linux__
#include <string.h>
#endif

/**
 * @brief Signal handler.
 *
 * @param signum Number of received signal.
 */
void sighandler(int signum);

/**
 * @brief Register signal handlers.
 */
void reg_sighandlers();

#endif /* COMMON_H_IS_INCLUDED */
