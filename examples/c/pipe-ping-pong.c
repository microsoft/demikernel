// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

// This should come first.
// Glibc macro to expose definitions corresponding to the POSIX.1-2008 base specification.
// See https://man7.org/linux/man-pages/man7/feature_test_macros.7.html.
#define _POSIX_C_SOURCE 200809L

/*====================================================================================================================*
 * Imports                                                                                                            *
 *====================================================================================================================*/

#include <arpa/inet.h>
#include <assert.h>
#include <demi/libos.h>
#include <demi/sga.h>
#include <demi/wait.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>

/*====================================================================================================================*
 * Constants                                                                                                          *
 *====================================================================================================================*/

/**
 * @brief Data size.
 */
#define DATA_SIZE 64

/**
 * @brief Maximum number of bytes to transfer.
 */
#define MAX_BYTES (DATA_SIZE * 1024)

/*====================================================================================================================*
 * sighandler()                                                                                                       *
 *====================================================================================================================*/

/**
 * @brief Signal handler.
 *
 * @param signum Number of received signal.
 */
static void sighandler(int signum)
{
    const char *signame = strsignal(signum);

    fprintf(stderr, "\nReceived %s signal\n", signame);
    fprintf(stderr, "Exiting...\n");

    exit(EXIT_SUCCESS);
}

/*====================================================================================================================*
 * push_wait()                                                                                                        *
 *====================================================================================================================*/

/**
 * @brief Pushes a scatter-gather array to a remote socket and waits for operation to complete.
 *
 * @param qd  Target queue descriptor.
 * @param sga Target scatter-gather array.
 * @param qr  Storage location for operation result.
 */
static void push_wait(int qd, demi_sgarray_t *sga, demi_qresult_t *qr)
{
    demi_qtoken_t qt = -1;

    /* Push data. */
    assert(demi_push(&qt, qd, sga) == 0);

    /* Wait push operation to complete. */
    assert(demi_wait(qr, qt, NULL) == 0);

    /* Parse operation result. */
    assert(qr->qr_opcode == DEMI_OPC_PUSH);
}

/*====================================================================================================================*
 * pop_wait()                                                                                                         *
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
 * @brief Memory queue echo server.
 *
 * @param argc Argument count.
 * @param argv Argument list.
 */
static void server(int argc, char *const argv[])
{
    int nbytes = 0;
    char name[1024];
    int pipeqd_rx = -1;
    int pipeqd_tx = -1;

    /* Initialize demikernel */
    assert(demi_init(argc, argv) == 0);

    /* Setup memory queues. */
    sprintf(name, "%s:rx", argv[2]);
    assert(demi_create_pipe(&pipeqd_rx, name) == 0);
    sprintf(name, "%s:tx", argv[2]);
    assert(demi_create_pipe(&pipeqd_tx, name) == 0);

    /* Run. */
    while (nbytes < MAX_BYTES)
    {
        demi_qresult_t qr = {0};
        demi_sgarray_t sga = {0};

        /* Pop scatter-gather array. */
        pop_wait(pipeqd_rx, &qr);

        /* Extract received scatter-gather array. */
        memcpy(&sga, &qr.qr_value.sga, sizeof(demi_sgarray_t));

        nbytes += sga.sga_segs[0].sgaseg_len;

        /* Push scatter-gather array. */
        push_wait(pipeqd_tx, &sga, &qr);

        /* Release received scatter-gather array. */
        assert(demi_sgafree(&sga) == 0);

        fprintf(stdout, "ping (%d)\n", nbytes);
    }
}

/*====================================================================================================================*
 * client()                                                                                                           *
 *====================================================================================================================*/

/**
 * @brief Memory queue echo client.
 *
 * @param argc Argument count.
 * @param argv Argument list.
 */
static void client(int argc, char *const argv[])
{
    int nbytes = 0;
    char name[1024];
    int pipeqd_tx = -1;
    int pipeqd_rx = -1;

    /* Initialize demikernel */
    assert(demi_init(argc, argv) == 0);

    /* Setup memory queues. */
    sprintf(name, "%s:tx", argv[2]);
    assert(demi_open_pipe(&pipeqd_rx, name) == 0);
    sprintf(name, "%s:rx", argv[2]);
    assert(demi_open_pipe(&pipeqd_tx, name) == 0);

    /* Run. */
    while (nbytes < MAX_BYTES)
    {
        demi_qresult_t qr = {0};
        demi_sgarray_t sga = {0};

        /* Allocate scatter-gather array. */
        sga = demi_sgaalloc(DATA_SIZE);
        assert(sga.sga_segs != 0);

        /* Cook data. */
        memset(sga.sga_segs[0].sgaseg_buf, 1, DATA_SIZE);

        /* Push scatter-gather array. */
        push_wait(pipeqd_tx, &sga, &qr);

        /* Release sent scatter-gather array. */
        assert(demi_sgafree(&sga) == 0);

        /* Pop data scatter-gather array. */
        memset(&qr, 0, sizeof(demi_qresult_t));
        pop_wait(pipeqd_rx, &qr);

        /* Check payload. */
        for (uint32_t i = 0; i < qr.qr_value.sga.sga_segs[0].sgaseg_len; i++)
            assert(((char *)qr.qr_value.sga.sga_segs[0].sgaseg_buf)[i] == 1);
        nbytes += qr.qr_value.sga.sga_segs[0].sgaseg_len;

        /* Release received scatter-gather array. */
        assert(demi_sgafree(&qr.qr_value.sga) == 0);

        fprintf(stdout, "pong (%d)\n", nbytes);
    }
}

/*====================================================================================================================*
 * usage()                                                                                                            *
 *====================================================================================================================*/

/**
 * @brief Prints program usage.
 *
 * @param progname Program name.
 */
static void usage(const char *progname)
{
    fprintf(stderr, "Usage: %s MODE pipe-name\n", progname);
    fprintf(stderr, "MODE:\n");
    fprintf(stderr, "  --client    Run in client mode.\n");
    fprintf(stderr, "  --server    Run in server mode.\n");
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
    /* Install signal handlers. */
    signal(SIGINT, sighandler);
    signal(SIGQUIT, sighandler);
    signal(SIGTSTP, sighandler);

    if (argc >= 3)
    {
        /* Run. */
        if (!strcmp(argv[1], "--server"))
            server(argc, argv);
        else if (!strcmp(argv[1], "--client"))
            client(argc, argv);

        return (EXIT_SUCCESS);
    }

    usage(argv[0]);

    return (EXIT_SUCCESS);
}
