#include <iostream>
#include <string.h>
#include <netinet/in.h>
#include <assert.h>
#include <arpa/inet.h>

#include "../include/io-queue.h"

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
    if (inet_pton(AF_INET, "10.0.0.5", &(server.sin_addr)) != 1) {
        printf("Address not supported!\n");
        return -1;
    }
    server.sin_port = htons(port);

    if (Zeus::bind(lqd, (struct sockaddr*)&server, sizeof(server)) < 0) {
        printf("Error binding queue!\n");
        return -1;
    }

    if (Zeus::listen(lqd, 3) < 0) {
        printf("Error listening!\n");
        return -1;
    }

    if (qd = Zeus::accept(lqd, (struct sockaddr*)&peer, &len) < 0) {
        printf("Error accepting connection\n");
        return -1;
    }
    
    qt = Zeus::pop(qd, sga);
    if (qt != 0) {
	printf("server: wait for pop\n");
	n = Zeus::wait(qt, sga);
	assert(n > 0);
    }

    assert(sga.num_bufs == 1);

    printf("server rcvd:\t%s\n", (char*)sga.bufs[0].buf);

    qt = Zeus::push(qd, sga);
    if (qt != 0) {
	printf("server: wait for push\n");
	n = Zeus::wait(qt, sga);
	assert(n > 0);
    }

    //printf("===========================\n");
    printf("server sent:\t%s\n", (char*)sga.bufs[0].buf);

    Zeus::close(qd);
    Zeus::close(lqd);

    return 0;
}
