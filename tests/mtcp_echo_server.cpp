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
#include "../libos/common/latency.h"

int main(int argc, char **argv) {
    mtcp_env_init();

    int lqd, qd;
    ssize_t n;
    struct sgarray sga;
    struct sockaddr_in server, peer;
    socklen_t len = sizeof(struct sockaddr_in);

    if ((lqd = MTCP_socket(AF_INET, SOCK_STREAM, 0)) < 0) {
        printf("Error creating socket!\n");
        return -1;
    }

    printf("server listen qd:\t%d\n", lqd);

    server.sin_family = AF_INET;
    server.sin_addr.s_addr = 0x040c0c0c;
    server.sin_port = htons(12345);

    if (MTCP_bind(lqd, (struct sockaddr*)&server, sizeof(server)) < 0) {
        printf("Error binding socket!\n");
        return -1;
    }

    printf("server: listening for connections\n");
    if (MTCP_listen(lqd, 3) < 0) {
        printf("Error listening\n");
        return -1;
    }

    while ((qd = MTCP_accept(lqd, (struct sockaddr*)&peer, &len)) <= 0) {
        if (qd < 0) {
            printf("Error accepting connection!\n");
            return -1;
        }
    }

    printf("server: accepted connection from: %x:%d\n", server.sin_addr.s_addr, server.sin_port);

    ssize_t ret;
    struct Pending* req;
    for (int i = 0; i < PKTNUM; i++) {
        req = new Pending(sga);
        while ((ret = MTCP_recv(qd, req)) <= 0) {
            if (ret < 0) {
                perror("server pop");
                return -1;
            }
        }
        assert(sga.num_bufs == 1);
        delete req;

        req = new Pending(sga);
        while ((ret = MTCP_send(qd, req)) <= 0) {
            if (ret < 0) {
                perror("");
                return -1;
            }
        }
        delete req;
    }

    Latency_DumpAll();

    MTCP_close(qd);
    MTCP_close(lqd);


	return 0;
}
