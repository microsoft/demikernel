#include <list>
#include <unordered_map>
#include <mtcp_api.h>
#include <mtcp_epoll.h>
#include <fcntl.h>
#include <unistd.h>
#include <arpa/inet.h>
#include <netinet/tcp.h>
#include <assert.h>
#include <string.h>
#include <errno.h>
#include <sys/time.h>
#include <stdint.h>
#include <stdlib.h>
#include <stdio.h>
#include <netinet/in.h>

#include "mtcp_common.h"

int main(int argc, char **argv) {
    mtcp_env_init();

    int qd;
    ssize_t n;
    struct sgarray sga, res;
    char *buf = (char*)malloc(BUFSIZE);
    struct sockaddr_in server;
    buf[0] = 0;

    if ((qd = MTCP_socket(AF_INET, SOCK_STREAM, 0)) < 0) {
        printf("Error creating socket!\n");
        return -1;
    }

    printf("client qd:\t%d\n", qd);

    server.sin_family = AF_INET;
    server.sin_addr.s_addr = 0x040c0c0c;
    server.sin_port = htons(12345);

    if (MTCP_connect(qd, (struct sockaddr*)&server, sizeof(struct sockaddr_in)) < 0) {
        perror("Error connecting queue:");
        return -1;
    }

    sga.num_bufs = 1;
    sga.bufs[0].len = BUFSIZE;
    sga.bufs[0].buf = (ioptr)buf;

    struct Pending* req;

    for (int i = 0; i < PKTNUM; i++) {
        req = new Pending(sga);
        while (MTCP_send(qd, req) <= 0);
        //printf("client sent:\t%s\n", (char*)sga.bufs[0].buf);
        delete req;

        req = new Pending(res);
        while ((n = MTCP_recv(qd, req)) <= 0) {
            if (n < 0) {
                perror("client pop");
                return -1;
            }
        }
        //printf("client rcvd:\t%s\n", (char*)res.bufs[0].buf);
        delete req;
        //printf("req #%d\n", i);
    }

    MTCP_close(qd);


    return 0;
}
