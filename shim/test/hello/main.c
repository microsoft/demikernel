// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#include <assert.h>
#include <stdlib.h>
#include <sys/socket.h>
#include <stdio.h>

// This is a test program that creates a socket and exits.
int main(int argc, char *const argv[])
{
    int sockfd = -1;

    ((void) argc);
    ((void) argv);

    sockfd = socket(AF_INET, SOCK_STREAM, 0);
    assert(sockfd != -1);

    fprintf(stdout, "socket created! (sockfd=%d)\n", sockfd);
    fprintf(stderr, "sockfd: %d\n", sockfd);

    return (EXIT_SUCCESS);
}
