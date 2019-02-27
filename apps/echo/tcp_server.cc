#include <dmtr/annot.h>
#include <dmtr/libos.h>
#include <libos/common/mem.h>
#include <dmtr/wait.h>
#include <libos/common/latency.h>

#include <arpa/inet.h>
#include <cassert>
#include <cstring>
#include <iostream>
#include <netinet/in.h>
#include <unistd.h>

#define ITERATION_COUNT 10000
static const uint16_t PORT = 12345;

int main()
{
    char *argv[] = {};
    DMTR_OK(dmtr_init(0, argv));

    int lqd = 0;
    DMTR_OK(dmtr_socket(&lqd, AF_INET, SOCK_STREAM, 0));
    printf("listen qd:\t%d\n", lqd);

    struct sockaddr_in saddr = {};
    saddr.sin_family = AF_INET;
    saddr.sin_addr.s_addr = INADDR_ANY;
    saddr.sin_port = PORT;
    DMTR_OK(dmtr_bind(lqd, reinterpret_cast<struct sockaddr *>(&saddr), sizeof(saddr)));

    printf("listening for connections\n");
    DMTR_OK(dmtr_listen(lqd, 3));

    dmtr_qtoken_t qt = 0;
    dmtr_qresult_t qr = {};
    DMTR_OK(dmtr_accept(&qt, lqd));
    DMTR_OK(dmtr_wait(&qr, qt));
    DMTR_OK(dmtr_drop(qt));
    DMTR_TRUE(EPERM, DMTR_OPC_ACCEPT == qr.qr_opcode);
    DMTR_TRUE(EPERM, DMTR_TID_QD == qr.qr_tid);
    int qd = qr.qr_value.qd;
    //printf("accepted connection from: %x:%d\n", paddr.sin_addr.s_addr, paddr.sin_port);

    // process ITERATION_COUNT packets from client
    for (size_t i = 0; i < ITERATION_COUNT; i++) {
        DMTR_OK(dmtr_pop(&qt, qd));
        DMTR_OK(dmtr_wait(&qr, qt));
        DMTR_OK(dmtr_drop(qt));
        DMTR_TRUE(EPERM, DMTR_OPC_POP == qr.qr_opcode);
        DMTR_TRUE(EPERM, DMTR_TID_SGA == qr.qr_tid);
        DMTR_TRUE(EPERM, qr.qr_value.sga.sga_numsegs == 1);

        fprintf(stderr, "[%lu] server: rcvd\t%s\tbuf size:\t%d\n", i, reinterpret_cast<char *>(qr.qr_value.sga.sga_segs[0].sgaseg_buf), qr.qr_value.sga.sga_segs[0].sgaseg_len);
        DMTR_OK(dmtr_push(&qt, qd, &qr.qr_value.sga));
        DMTR_OK(dmtr_wait(NULL, qt));
        DMTR_OK(dmtr_drop(qt));

        fprintf(stderr, "send complete.\n");
        free(qr.qr_value.sga.sga_buf);
    }

    Latency_DumpAll();
    DMTR_OK(dmtr_close(qd));
    DMTR_OK(dmtr_close(lqd));

    return 0;
}
