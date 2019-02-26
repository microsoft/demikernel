#include <dmtr/annot.h>
#include <dmtr/libos.h>
#include <dmtr/mem.h>
#include <dmtr/wait.h>

#include <iostream>
#include <cstring>
#include <netinet/in.h>
#include <cassert>
#include <arpa/inet.h>

#define USE_CONNECT 1
#define ITERATION_COUNT 10000
#define BUFFER_SIZE 10
#define FILL_CHAR 'a'
static const uint16_t PORT = 12345;

int main()
{
    char *argv[] = {};
    DMTR_OK(dmtr_init(0, argv));

    int qd = 0;
    DMTR_OK(dmtr_socket(&qd, AF_INET, SOCK_DGRAM, 0));
    printf("client qd:\t%d\n", qd);

    struct sockaddr_in saddr = {};
    saddr.sin_family = AF_INET;
    saddr.sin_port = PORT;
    if (inet_pton(AF_INET, "10.0.0.7", &(saddr.sin_addr)) != 1) {
        printf("Address not supported!\n");
        return -1;
    }

    printf("client: sending to: %x:%d\n", saddr.sin_addr.s_addr, saddr.sin_port);

    dmtr_sgarray_t sga = {};
    void *p = NULL;
    DMTR_OK(dmtr_malloc(&p, BUFFER_SIZE));
    char *s = reinterpret_cast<char *>(p);
    memset(s, FILL_CHAR, BUFFER_SIZE);
    s[BUFFER_SIZE - 1] = '\0';
    sga.sga_numsegs = 1;
    sga.sga_segs[0].sgaseg_len = BUFFER_SIZE;
    sga.sga_segs[0].sgaseg_buf = p;

#if USE_CONNECT
    DMTR_OK(dmtr_connect(qd, (struct sockaddr*)&saddr, sizeof(saddr)));
#else
    sga.sga_addr = saddr;
    sga.sga_addrlen = sizeof(saddr);
#endif

    for (size_t i = 0; i < ITERATION_COUNT; i++) {
        dmtr_qtoken_t qt;
        DMTR_OK(dmtr_push(&qt, qd, &sga));
        DMTR_OK(dmtr_wait(NULL, qt));
        DMTR_OK(dmtr_drop(qt));
        fprintf(stderr, "send complete.\n");

        dmtr_qresult_t qr = {};
        DMTR_OK(dmtr_pop(&qt, qd));
        DMTR_OK(dmtr_wait(&qr, qt));
        DMTR_OK(dmtr_drop(qt));
        DMTR_TRUE(EPERM, DMTR_OPC_POP == qr.qr_opcode);
        DMTR_TRUE(EPERM, DMTR_TID_SGA == qr.qr_tid);
        DMTR_TRUE(EPERM, qr.qr_value.sga.sga_numsegs == 1);
        DMTR_TRUE(EPERM, reinterpret_cast<uint8_t *>(qr.qr_value.sga.sga_segs[0].sgaseg_buf)[0] == FILL_CHAR);

        fprintf(stderr, "[%lu] client: rcvd\t%s\tbuf size:\t%d\n", i, reinterpret_cast<char *>(qr.qr_value.sga.sga_segs[0].sgaseg_buf), qr.qr_value.sga.sga_segs[0].sgaseg_len);
        free(qr.qr_value.sga.sga_buf);
    }

    DMTR_OK(dmtr_close(qd));

    return 0;
}
