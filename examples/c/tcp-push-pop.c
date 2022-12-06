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

#define DATA_SIZE 64
#define MAX_BYTES DATA_SIZE * 1024

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

static void run_server(struct sockaddr_in *addr)
{
    unsigned int recv_bytes = 0;
    int sockqd = accept_get_newsockqd(addr);

    while (recv_bytes < MAX_BYTES)
    {
        recv_bytes += pop_get_received_nbytes(sockqd);
        fprintf(stdout, "pop: total bytes received: (%d)\n", recv_bytes);
    }

    assert(recv_bytes == MAX_BYTES);
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

static int push_get_sent_nbytes(const struct sockaddr_in *const addr, const int sockqd)
{
    demi_qtoken_t tok = -1;
    demi_qresult_t res = {0};
    demi_sgarray_t sga = demi_sgaalloc(DATA_SIZE);
    int sent_bytes = 0;

    assert(sga.sga_segs != 0);
    memset(sga.sga_segs[0].sgaseg_buf, 1, DATA_SIZE);
    assert(demi_pushto(&tok, sockqd, &sga, (const struct sockaddr *)addr, sizeof(struct sockaddr_in)) == 0);
    assert(demi_wait(&res, tok, NULL) == 0);
    assert(res.qr_opcode == DEMI_OPC_PUSH);
    sent_bytes = sga.sga_segs[0].sgaseg_len;
    assert(demi_sgafree(&sga) == 0);
    return sent_bytes;
}

static void run_client(const struct sockaddr_in *const addr)
{
    unsigned int sent_bytes = 0;
    int sockqd = connect_get_sockqd(addr);

    while (sent_bytes < MAX_BYTES)
    {
        sent_bytes += push_get_sent_nbytes(addr, sockqd);
        fprintf(stdout, "push: total bytes sent: (%d)\n", sent_bytes);
    }

    assert(sent_bytes == MAX_BYTES);
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
int main(int argc, const char *argv[])
{
    struct sockaddr_in addr = {0};

    if (argc != 4)
    {
        usage(argv[0]);
        return (EXIT_FAILURE);
    }

    reg_sighandlers();
    assert(demi_init(argc, (char **)argv) == 0);
    build_addr(argv[2], argv[3], &addr);

    if (!strcmp(argv[1], "--server"))
    {
        run_server(&addr);
    }
    else if (!strcmp(argv[1], "--client"))
    {
        run_client(&addr);
    }
    else
    {
        usage(argv[0]);
        return (EXIT_FAILURE);
    }

    return (EXIT_SUCCESS);
}
