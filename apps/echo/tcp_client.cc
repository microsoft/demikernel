#include <dmtr/annot.h>
#include <dmtr/libos.h>
#include <dmtr/mem.h>
#include <dmtr/wait.h>

#include <arpa/inet.h>
#include <cstring>
#include <iostream>
#include <netinet/in.h>

#define ITERATION_COUNT 10
#define BUFFER_SIZE 10
#define FILL_CHAR 'a'
static const uint16_t PORT = 12345;

int main()
{
    char *argv[] = {};
    DMTR_OK(dmtr_init(0, argv));

    int qd = 0;
    DMTR_OK(dmtr_socket(&qd, AF_INET, SOCK_STREAM, 0));
    printf("client qd:\t%d\n", qd);

    struct sockaddr_in saddr;
    saddr.sin_family = AF_INET;
    //if (inet_pton(AF_INET, "192.168.1.2", &saddr.sin_addr) != 1) {
    if (inet_pton(AF_INET, "10.10.1.1", &saddr.sin_addr) != 1) {
        printf("Address not supported!\n");
        return -1;
    }
    saddr.sin_port = htons(PORT);
    DMTR_OK(dmtr_connect(qd, reinterpret_cast<struct sockaddr *>(&saddr), sizeof(saddr)));

    dmtr_sgarray_t sga = {};
    void *p = NULL;
    DMTR_OK(dmtr_malloc(&p, BUFFER_SIZE));
    char *s = reinterpret_cast<char *>(p);
    memset(s, FILL_CHAR, BUFFER_SIZE);
    s[BUFFER_SIZE - 1] = '\0';
    sga.sga_numsegs = 1;
    sga.sga_segs[0].sgaseg_len = BUFFER_SIZE;
    sga.sga_segs[0].sgaseg_buf = p;

    for (size_t i = 0; i < ITERATION_COUNT; i++) {
        dmtr_qtoken_t qt;
        DMTR_OK(dmtr_push(&qt, qd, &sga));
        DMTR_OK(dmtr_wait(NULL, qt));
        //DMTR_OK(dmtr_drop(qt));
        fprintf(stderr, "send complete.\n");

        dmtr_sgarray_t recvd;
        DMTR_OK(dmtr_pop(&qt, qd));
        DMTR_OK(dmtr_wait(&recvd, qt));
        DMTR_OK(dmtr_drop(qt));
        DMTR_TRUE(EPERM, recvd.sga_numsegs == 1);
        DMTR_TRUE(EPERM, reinterpret_cast<uint8_t *>(recvd.sga_segs[0].sgaseg_buf)[0] == FILL_CHAR);

        fprintf(stderr, "[%lu] client: rcvd\t%s\tbuf size:\t%d\n", i, reinterpret_cast<char *>(recvd.sga_segs[0].sgaseg_buf), recvd.sga_segs[0].sgaseg_len);
        free(recvd.sga_buf);
    }

    DMTR_OK(dmtr_close(qd));

    return 0;
}
