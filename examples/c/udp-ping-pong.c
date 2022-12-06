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
#include <signal.h>
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
 * push_and_wait()                                                                                                    *
 *====================================================================================================================*/

/**
 * @brief Pushes a scatter-gather array to a remote socket and waits for operation to complete.
 *
 * @param qd   Target queue descriptor.
 * @param sga  Target scatter-gather array.
 * @param qr   Storage location for operation result.
 * @param dest Destination address.
 */
static void pushto_wait(int qd, demi_sgarray_t *sga, demi_qresult_t *qr, const struct sockaddr *dest)
{
    demi_qtoken_t qt = -1;

    /* Push data. */
    assert(demi_pushto(&qt, qd, sga, (const struct sockaddr *)dest, sizeof(struct sockaddr_in)) == 0);

    /* Wait push operation to complete. */
    assert(demi_wait(qr, qt, NULL) == 0);

    /* Parse operation result. */
    assert(qr->qr_opcode == DEMI_OPC_PUSH);
}

/*====================================================================================================================*
 * pop_and_wait()                                                                                                    *
 *====================================================================================================================*/

/**
 * @brief Pops a scatter-gather array and waits for operation to complete.
 *
 * @param qd Target queue descriptor.
 * @param qr Storage location for operation result.
 */
static void pop_wait(int qd, demi_qresult_t *qr)
{
    demi_qtoken_t qt = -1;

    /* Pop data. */
    assert(demi_pop(&qt, qd) == 0);

    /* Wait for pop operation to complete. */
    assert(demi_wait(qr, qt, NULL) == 0);

    /* Parse operation result. */
    assert(qr->qr_opcode == DEMI_OPC_POP);
    assert(qr->qr_value.sga.sga_segs != 0);
}

/*====================================================================================================================*
 * server()                                                                                                           *
 *====================================================================================================================*/

/**
 * @brief UDP echo server.
 *
 * @param argc   Argument count.
 * @param argv   Argument list.
 * @param local  Local socket address.
 * @param remote Remote socket address.
 */
static void server(int argc, char *const argv[], struct sockaddr_in *local, struct sockaddr_in *remote)
{
    int sockqd = -1;

    /* Initialize demikernel */
    assert(demi_init(argc, argv) == 0);

    /* Setup local socket. */
    assert(demi_socket(&sockqd, AF_INET, SOCK_DGRAM, 0) == 0);
    assert(demi_bind(sockqd, (const struct sockaddr *)local, sizeof(struct sockaddr_in)) == 0);

    /* Run. */
    for (int it = 0; it < MAX_ITERATIONS; it++)
    {
        demi_qresult_t qr = {0};
        demi_sgarray_t sga = {0};

        /* Pop scatter-gather array. */
        pop_wait(sockqd, &qr);

        /* Extract received scatter-gather array. */
        memcpy(&sga, &qr.qr_value.sga, sizeof(demi_sgarray_t));

        /* Push scatter-gather array. */
        pushto_wait(sockqd, &sga, &qr, (const struct sockaddr *)remote);

        /* Release received scatter-gather array. */
        assert(demi_sgafree(&sga) == 0);

        fprintf(stdout, "ping (%d)\n", it);
    }
}

/*====================================================================================================================*
 * client()                                                                                                           *
 *====================================================================================================================*/

/**
 * @brief UDP echo client.
 *
 * @param argc   Argument count.
 * @param argv   Argument list.
 * @param local  Local socket address.
 * @param remote Remote socket address.
 */
static void client(int argc, char *const argv[], struct sockaddr_in *local, struct sockaddr_in *remote)
{
    int sockqd = -1;
    char expected_buf[DATA_SIZE];

    /* Initialize demikernel */
    assert(demi_init(argc, argv) == 0);

    /* Setup socket. */
    assert(demi_socket(&sockqd, AF_INET, SOCK_DGRAM, 0) == 0);
    assert(demi_bind(sockqd, (const struct sockaddr *)local, sizeof(struct sockaddr_in)) == 0);

    /* Run. */
    for (int it = 0; it < MAX_ITERATIONS; it++)
    {
        demi_qresult_t qr = {0};
        demi_sgarray_t sga = {0};

        /* Allocate scatter-gather array. */
        sga = demi_sgaalloc(DATA_SIZE);
        assert(sga.sga_segs != 0);

        /* Cook data. */
        memset(expected_buf, it % 256, DATA_SIZE);
        memcpy(sga.sga_segs[0].sgaseg_buf, expected_buf, DATA_SIZE);

        /* Push scatter-gather array. */
        pushto_wait(sockqd, &sga, &qr, (const struct sockaddr *)remote);

        /* Release sent scatter-gather array. */
        assert(demi_sgafree(&sga) == 0);

        /* Pop data scatter-gather array. */
        pop_wait(sockqd, &qr);

        /* Parse operation result. */
        assert(!memcmp(qr.qr_value.sga.sga_segs[0].sgaseg_buf, expected_buf, DATA_SIZE));

        /* Release received scatter-gather array. */
        assert(demi_sgafree(&qr.qr_value.sga) == 0);

        fprintf(stdout, "pong (%d)\n", it);
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
    fprintf(stderr, "Usage: %s MODE local-ipv4 local-port remote-ipv4 remote-port\n", progname);
    fprintf(stderr, "Modes:\n");
    fprintf(stderr, "  --client    Run program in client mode.\n");
    fprintf(stderr, "  --server    Run program in server mode.\n");

    exit(EXIT_SUCCESS);
}

/*====================================================================================================================*
 * build_sockaddr()                                                                                                   *
 *====================================================================================================================*/

/**
 * @brief Builds a socket address.
 *
 * @param ip_str    String representation of an IP address.
 * @param port_str  String representation of a port number.
 * @param addr      Storage location for socket address.
 */
void build_sockaddr(const char *const ip_str, const char *const port_str, struct sockaddr_in *const addr)
{
    int port = -1;

    sscanf(port_str, "%d", &port);
    addr->sin_family = AF_INET;
    addr->sin_port = htons(port);
    assert(inet_pton(AF_INET, ip_str, &addr->sin_addr) == 1);
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
    if (argc >= 6)
    {
        reg_sighandlers();

        struct sockaddr_in local = {0};
        struct sockaddr_in remote = {0};

        /* Build local addresses.*/
        build_sockaddr(argv[2], argv[3], &local);
        build_sockaddr(argv[4], argv[5], &remote);

        if (!strcmp(argv[1], "--server"))
        {
            server(argc, argv, &local, &remote);
            return (EXIT_SUCCESS);
        }
        else if (!strcmp(argv[1], "--client"))
        {
            client(argc, argv, &local, &remote);
            return (EXIT_SUCCESS);
        }
    }

    usage(argv[0]);

    /* Never gets here. */

    return (EXIT_SUCCESS);
}
