#include <iostream>
#include <string.h>
#include <netinet/in.h>
#include <assert.h>
#include <arpa/inet.h>
#include <unistd.h>

#include "../include/io-queue.h"
#include "../libos/common/latency.h"

#define PKTNUM		10000

uint16_t port = 12345;

int main()
{
    int lqd, qd;
    ssize_t n;
    Zeus::qtoken qt;
    struct Zeus::sgarray sga;
    struct sockaddr_in server, peer;
    socklen_t len = sizeof(struct sockaddr_in);

    if ((lqd = Zeus::socket(AF_INET, SOCK_STREAM, 0)) < 0) {
        printf("Error creating queue!\n");
        return -1;
    }

    printf("server listen qd:\t%d\n", lqd);


    server.sin_family = AF_INET;
    server.sin_addr.s_addr = INADDR_ANY;
    server.sin_port = htons(port);

    if (Zeus::bind(lqd, (struct sockaddr*)&server, sizeof(server)) < 0) {
        printf("Error binding queue!\n");
        return -1;
    }

    printf("server: listening for connections\n");
    if (Zeus::listen(lqd, 3) < 0) {
        printf("Error listening!\n");
        return -1;
    }

    qt = Zeus::pop(lqd, sga);
    if (qt == 0) qd = Zeus::accept(lqd, (struct sockaddr*)&peer, &len);
    else {
        //printf("wait on accept: qt: %d\n", qt);
        Zeus::wait(qt, sga);
        qd = Zeus::accept(lqd, (struct sockaddr*)&peer, &len);
    }
    printf("server: accepted connection from: %x:%d\n", peer.sin_addr.s_addr, peer.sin_port);

    // process PKTNUM packets from client
    for (int i = 0; i < PKTNUM; i++) {
        while((n = Zeus::peek(qd, sga)) <= 0) {
            if (n < 0) {
                perror("server pop");
                return -1;
            }
        }

        assert(sga.num_bufs == 1);

        //printf("server rcvd:\t%s\n", (char*)sga.bufs[0].buf);

        qt = Zeus::push(qd, sga);
        if (qt != 0) {
            if (qt < 0) {
                perror("server push:");
                return -1;
            }
            n = Zeus::wait(qt, sga);
	        assert(n > 0);
        }

        //printf("===========================\n");
        //printf("server sent:\t%s\n", (char*)sga.bufs[0].buf);
        free(sga.bufs[0].buf);
    }

    Latency_DumpAll();
    Zeus::close(qd);
    Zeus::close(lqd);

    return 0;
}
