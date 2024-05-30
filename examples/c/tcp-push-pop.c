// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

// This should come first.
// Glibc macro to expose definitions corresponding to the POSIX.1-2008 base specification.
// See https://man7.org/linux/man-pages/man7/feature_test_macros.7.html.
#define _POSIX_C_SOURCE 200809L

#ifdef __linux__
#include <arpa/inet.h>
#endif

#include <assert.h>
#include <demi/libos.h>
#include <demi/sga.h>
#include <demi/wait.h>
#include <string.h>

#include "common.h"

/**
 * @brief Data size.
 */
#define DATA_SIZE 64

/**
 * @brief Maximum number of messages to transfer.
 */
#define MAX_MSGS 1024

static int accept_get_newsockqd(struct sockaddr_in *addr)
{
    int sockqd = -1;
    demi_qtoken_t tok = -1;
    demi_qresult_t res = {0};

    assert(demi_socket(&sockqd, AF_INET, SOCK_STREAM, 0) == 0);
    assert(demi_bind(sockqd, (const struct sockaddr *)addr, sizeof(struct sockaddr_in)) == 0);
    assert(demi_listen(sockqd, 16) == 0);
    assert(demi_accept(&tok, sockqd) == 0);
    assert(demi_wait(&res, tok, NULL) == 0);
    assert(res.qr_opcode == DEMI_OPC_ACCEPT);
    return res.qr_value.ares.qd;
}

static int pop_get_received_nbytes(int sockqd)
{
    demi_qresult_t res = {0};
    demi_qtoken_t tok = -1;
    int recv_bytes = 0;

    assert(demi_pop(&tok, sockqd) == 0);
    assert(demi_wait(&res, tok, NULL) == 0);
    assert(res.qr_opcode == DEMI_OPC_POP);
    assert(res.qr_value.sga.sga_segs != 0);
    recv_bytes = res.qr_value.sga.sga_segs[0].sgaseg_len;
    assert(demi_sgafree(&res.qr_value.sga) == 0);
    return recv_bytes;
}

static void run_server(struct sockaddr_in *addr, size_t data_size, unsigned max_msgs)
{
    size_t recv_bytes = 0;
    int sockqd = accept_get_newsockqd(addr);
    size_t max_bytes = data_size * max_msgs;

    while (recv_bytes < max_bytes)
    {
        recv_bytes += pop_get_received_nbytes(sockqd);
        fprintf(stdout, "pop: total bytes received: (%zu)\n", recv_bytes);
    }
}

static int connect_get_sockqd(const struct sockaddr_in *const addr)
{
    int sockqd = -1;
    demi_qtoken_t tok = -1;
    demi_qresult_t res = {0};

    assert(demi_socket(&sockqd, AF_INET, SOCK_STREAM, 0) == 0);
    assert(demi_connect(&tok, sockqd, (const struct sockaddr *)addr, sizeof(struct sockaddr_in)) == 0);
    assert(demi_wait(&res, tok, NULL) == 0);
    assert(res.qr_opcode == DEMI_OPC_CONNECT);
    return sockqd;
}

static int push_get_sent_nbytes(const int sockqd, size_t data_size)
{
    demi_qtoken_t tok = -1;
    demi_qresult_t res = {0};
    demi_sgarray_t sga = demi_sgaalloc(data_size);
    int sent_bytes = 0;

    assert(sga.sga_segs != 0);
    memset(sga.sga_segs[0].sgaseg_buf, 1, data_size);
    // ToDo: demi_pushto() also must work for TCP on all LibOSes.
    // FIXME: https://github.com/microsoft/demikernel/issues/137
    assert(demi_push(&tok, sockqd, &sga) == 0);
    assert(demi_wait(&res, tok, NULL) == 0);
    assert(res.qr_opcode == DEMI_OPC_PUSH);
    sent_bytes = sga.sga_segs[0].sgaseg_len;
    assert(demi_sgafree(&sga) == 0);
    return sent_bytes;
}

static void run_client(const struct sockaddr_in *const addr, size_t data_size, unsigned max_msgs)
{
    size_t sent_bytes = 0;
    int sockqd = connect_get_sockqd(addr);
    size_t max_bytes = data_size * max_msgs;

    while (sent_bytes < max_bytes)
    {
        sent_bytes += push_get_sent_nbytes(sockqd, data_size);
        fprintf(stdout, "push: total bytes sent: (%zu)\n", sent_bytes);
    }
}

static void usage(const char *const progname)
{
    fprintf(stderr, "Usage: %s MODE ipv4-address port\n", progname);
    fprintf(stderr, "MODE:\n");
    fprintf(stderr, "  --client    Run in client mode.\n");
    fprintf(stderr, "  --server    Run in server mode.\n");
}

void build_addr(const char *const ip_str, const char *const port_str, struct sockaddr_in *const addr)
{
    int port = -1;

    sscanf(port_str, "%d", &port);
    addr->sin_family = AF_INET;
    addr->sin_port = htons(port);
    assert(inet_pton(AF_INET, ip_str, &addr->sin_addr) == 1);
}

// Exercises a one-way direction communication through TCP. This system-level test instantiates two demikernel peers: a
// client and a server. The client sends TCP packets to the server in a tight loop. The server process is a tight loop
// received TCP packets from the client.
int main(int argc, char *const argv[])
{
    if (argc >= 4)
    {
        reg_sighandlers();

        struct sockaddr_in addr = {0};
        size_t data_size = DATA_SIZE;
        unsigned max_msgs = MAX_MSGS;

        if (argc >= 5)
            sscanf(argv[4], "%zu", &data_size);
        if (argc >= 6)
            sscanf(argv[5], "%u", &max_msgs);

        build_addr(argv[2], argv[3], &addr);

        const struct demi_args args = {
            .argc = argc,
            .argv = argv,
            .callback = NULL,
        };
        assert(demi_init(&args) == 0);

        if (!strcmp(argv[1], "--server")) {
            run_server(&addr, data_size, max_msgs);
        } else if (!strcmp(argv[1], "--client")) {
            run_client(&addr, data_size, max_msgs);
        }

        return (EXIT_SUCCESS);
    }

    usage(argv[0]);

    return (EXIT_FAILURE);
}
