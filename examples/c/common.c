// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#include "common.h"

/**
 * @brief Signal handler.
 *
 * @param signum Number of received signal.
 */
void sighandler(int signum)
{
    #ifdef __linux__
    const char *signame = strsignal(signum);
    fprintf(stderr, "\nReceived %s signal\n", signame);
    #endif

    #ifdef _WIN32
    fprintf(stderr, "\nReceived %d signal\n", signum);
    #endif

    fprintf(stderr, "Exiting...\n");
    exit(EXIT_SUCCESS);
}

/**
 * @brief Register signal handlers.
 */
void reg_sighandlers()
{
    signal(SIGINT, sighandler);

    #ifdef __linux__
    signal(SIGQUIT, sighandler);
    signal(SIGTSTP, sighandler);
    #endif
}
