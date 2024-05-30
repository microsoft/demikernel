// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

// This should come first.
// Glibc macro to expose definitions corresponding to the POSIX.1-2008 base specification.
// See https://man7.org/linux/man-pages/man7/feature_test_macros.7.html.
#define _POSIX_C_SOURCE 200809L

/*====================================================================================================================*
 * Imports                                                                                                            *
 *====================================================================================================================*/

#include <assert.h>
#include <demi/libos.h>
#include <demi/sga.h>
#include <demi/wait.h>
#include <string.h>

#ifdef __linux__
#include <arpa/inet.h>
#include <sys/socket.h>
#endif

#include "common.h"

/*====================================================================================================================*
 * Constants                                                                                                          *
 *====================================================================================================================*/

/**
 * @brief Data size.
 */
#define DATA_SIZE 64

/**
 * @brief Maximum number of iterations.
 */
#define MAX_ITERATIONS 1000000

/*====================================================================================================================*
 * server()                                                                                                           *
 *====================================================================================================================*/

/**
 * @brief UDP server.
 *
 * @param argc   Argument count.
 * @param argv   Argument list.
 * @param local  Local socket address.
 * @param remote Remote socket address.
 * @param data_size Number of bytes in each message.
 * @param max_iterations Maximum number of iterations.
 */
static void server(int argc,
                   char *const argv[],
                   struct sockaddr_in *local,
                   size_t data_size,
                   unsigned max_iterations)
{
    int sockqd = -1;

#ifdef _WIN32
    char expected_buf[DATA_SIZE];
#endif

#ifdef __linux__
    char expected_buf[data_size];
#endif

    /* Initialize demikernel */
    const struct demi_args args = {
        .argc = argc,
        .argv = argv,
        .callback = NULL,
    };
    assert(demi_init(&args) == 0);

    /* Setup socket. */
    assert(demi_socket(&sockqd, AF_INET, SOCK_DGRAM, 0) == 0);
    assert(demi_bind(sockqd, (const struct sockaddr *)local, sizeof(struct sockaddr_in)) == 0);

    memset(expected_buf, 1, data_size);

    /* Run. */
    for (unsigned it = 0; it < max_iterations; it++)
    {
        demi_qtoken_t qt = -1;
        demi_qresult_t qr = {0};

        /* Pop data. */
        assert(demi_pop(&qt, sockqd) == 0);

        /* Wait for pop operation to complete. */
        assert(demi_wait(&qr, qt, NULL) == 0);

        /* Parse operation result. */
        assert(qr.qr_opcode == DEMI_OPC_POP);
        assert(qr.qr_value.sga.sga_segs != 0);
        assert(!memcmp(qr.qr_value.sga.sga_segs[0].sgaseg_buf, expected_buf, data_size));

        /* Release scatter-gather array. */
        assert(demi_sgafree(&qr.qr_value.sga) == 0);

        fprintf(stdout, "pop (%u)\n", it);
    }
}

/*====================================================================================================================*
 * client()                                                                                                           *
 *====================================================================================================================*/

/**
 * @brief UDP client.
 *
 * @param argc   Argument count.
 * @param argv   Argument list.
 * @param local  Local socket address.
 * @param remote Remote socket address.
 * @param data_size Number of bytes in each message.
 * @param max_iterations Maximum number of iterations.
 */
static void client(int argc,
                   char *const argv[],
                   struct sockaddr_in *local,
                   struct sockaddr_in *remote,
                   size_t data_size,
                   unsigned max_iterations)
{
    int sockqd = -1;

    /* Initialize demikernel */
    const struct demi_args args = {
        .argc = argc,
        .argv = argv,
        .callback = NULL,
    };
    assert(demi_init(&args) == 0);

    /* Setup socket. */
    assert(demi_socket(&sockqd, AF_INET, SOCK_DGRAM, 0) == 0);
    assert(demi_bind(sockqd, (const struct sockaddr *)local, sizeof(struct sockaddr_in)) == 0);

    /* Run. */
    for (unsigned it = 0; it < max_iterations; it++)
    {
        demi_qtoken_t qt = -1;
        demi_qresult_t qr = {0};
        demi_sgarray_t sga = {0};

        /* Allocate scatter-gather array. */
        sga = demi_sgaalloc(data_size);
        assert(sga.sga_segs != 0);

        /* Cook data. */
        memset(sga.sga_segs[0].sgaseg_buf, 1, data_size);

        /* Push data. */
        assert(demi_pushto(&qt, sockqd, &sga, (const struct sockaddr *)remote, sizeof(struct sockaddr_in)) == 0);

        /* Wait push operation to complete. */
        assert(demi_wait(&qr, qt, NULL) == 0);

        /* Parse operation result. */
        assert(qr.qr_opcode == DEMI_OPC_PUSH);

        /* Release scatter-gather array. */
        assert(demi_sgafree(&sga) == 0);

        fprintf(stdout, "push (%u)\n", it);
    }
}

/*====================================================================================================================*
 * usage()                                                                                                            *
 *====================================================================================================================*/

/**
 * @brief Prints program usage and exits.
 *
 * @param progname Program name.
 */
static void usage(const char *progname)
{
    fprintf(stderr, "Usage: %s MODE local-ipv4 local-port [remote-ipv4] [remote-port]\n", progname);
    fprintf(stderr, "Modes:\n");
    fprintf(stderr, "  --client    Run program in client mode.\n");
    fprintf(stderr, "  --server    Run program in server mode.\n");

    exit(EXIT_SUCCESS);
}

/*====================================================================================================================*
 * main()                                                                                                             *
 *====================================================================================================================*/

/**
 * @brief Exercises a one-way direction communication through UDP.
 *
 * This system-level test instantiates two demikernel nodes: a client and a server. The client sends UDP packets to the
 * server in a tight loop. The server process in a tight loop received UDP packets from the client.
 *
 * @param argc Argument count.
 * @param argv Argument list.
 *
 * @return On successful completion EXIT_SUCCESS is returned.
 */
int main(int argc, char *const argv[])
{
    if (argc >= 4)
    {
        reg_sighandlers();

        int local_port = 0;
        struct sockaddr_in local = {0};
        size_t data_size = DATA_SIZE;
        unsigned max_iterations = MAX_ITERATIONS;

        /* Build local address.*/
        local.sin_family = AF_INET;
        sscanf(argv[3], "%d", &local_port);
        local.sin_port = htons(local_port);
        assert(inet_pton(AF_INET, argv[2], &local.sin_addr) == 1);

        if (!strcmp(argv[1], "--server"))
        {

            if (argc >= 5)
                sscanf(argv[4], "%zu", &data_size);
            if (argc >= 6)
                sscanf(argv[5], "%u", &max_iterations);

            server(argc, argv, &local, data_size, max_iterations);

            return (EXIT_SUCCESS);
        }
        else if ((argc >= 6) && (!strcmp(argv[1], "--client")))
        {
            int remote_port = 0;
            const char *remote_addr = argv[4];
            struct sockaddr_in remote = {0};

            if (argc >= 7)
                sscanf(argv[6], "%zu", &data_size);
            if (argc >= 8)
                sscanf(argv[7], "%u", &max_iterations);

            /* Build remote address. */
            remote.sin_family = AF_INET;
            sscanf(argv[5], "%d", &remote_port);
            remote.sin_port = htons(remote_port);
            assert(inet_pton(AF_INET, remote_addr, &remote.sin_addr) == 1);

            client(argc, argv, &local, &remote, data_size, max_iterations);

            return (EXIT_SUCCESS);
        }
    }

    usage(argv[0]);

    /* Never gets here. */

    return (EXIT_SUCCESS);
}
