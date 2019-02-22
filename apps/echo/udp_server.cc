#include <dmtr/annot.h>
#include <dmtr/libos.h>
#include <dmtr/mem.h>
#include <dmtr/wait.h>
#include <libos/common/latency.h>

#include <arpa/inet.h>
#include <cassert>
#include <cstring>
#include <iostream>
#include <netinet/in.h>

#define ITERATION_COUNT 10000
static const uint16_t PORT = 12345;

int main()
{
    char *argv[] = {};
    DMTR_OK(dmtr_init(0, argv));

    int qd = 0;
    DMTR_OK(dmtr_socket(&qd, AF_INET, SOCK_DGRAM, 0));
    printf("server qd:\t%d\n", qd);

    struct sockaddr_in saddr = {};
    saddr.sin_family = AF_INET;
    saddr.sin_port = PORT;
    if (inet_pton(AF_INET, "12.12.12.4", &(saddr.sin_addr)) != 1) {
        printf("Address not supported!\n");
        return -1;
    }

    DMTR_OK(dmtr_bind(qd, reinterpret_cast<struct sockaddr *>(&saddr), sizeof(saddr)));

    for (size_t i = 0; i < ITERATION_COUNT; i++) {
        dmtr_qresult_t qr = {};
        dmtr_qtoken_t qt = 0;
        DMTR_OK(dmtr_pop(&qt, qd));
        DMTR_OK(dmtr_wait(&qr, qt));
        DMTR_OK(dmtr_drop(qt));
        DMTR_TRUE(EPERM, DMTR_QR_SGA == qr.qr_tid);
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

    return 0;
}
