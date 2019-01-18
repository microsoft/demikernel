#include <iostream>
#include <string.h>
#include <netinet/in.h>
#include <assert.h>
#include <arpa/inet.h>

#include <zeus/io-queue.h>

#define PKTNUM		10000
#define BUFSIZE     10

uint16_t port = 12345;

int main()
{
    int qd;
    ssize_t n;
    Zeus::qtoken qt;
    struct Zeus::sgarray sga, res;
    char* buf = (char*)malloc(BUFSIZE);
    struct sockaddr_in server;

    buf[0] = 0;

    if ((qd = Zeus::socket(AF_INET, SOCK_STREAM, 0)) < 0) {
        printf("Error creating queue!\n");
        return -1;
    }

    printf("client qd:\t%d\n", qd);

    server.sin_family = AF_INET;
    if (inet_pton(AF_INET, "192.168.1.1", &server.sin_addr) != 1) {
        printf("Address not supported!\n");
        return -1;
    }
    server.sin_port = htons(port);

    while (Zeus::connect(qd, (struct sockaddr*)&server, sizeof(server)) < 0) {
        if (errno != EAGAIN && errno != EWOULDBLOCK) {
            perror("Error connecting queue:");
            return -1;
        }
    }

    sga.num_bufs = 1;
    sga.bufs[0].len = BUFSIZE;
    sga.bufs[0].buf = (Zeus::ioptr)buf;

    for (int i = 0; i < PKTNUM; i++) {
        qt = Zeus::push(qd, sga);
        if (qt != 0) {
            if (qt < 0) {
                perror("client push:");
                return -1;
            }
            //printf("client wait for push\n");
            n = Zeus::wait(qt, sga);
            assert(n > 0);
        }

        //printf("client: sent\t%s\tbuf size:\t%d\n", (char*)sga.bufs[0].buf, sga.bufs[0].len);

        while ((n = Zeus::peek(qd, res)) <= 0) {
            if (n < 0) {
                perror("client pop:");
                return -1;
            }
        }

        assert(res.num_bufs == 1);
        //fprintf(stderr, "client: rcvd\t%s\tbuf size:\t%d\n", (char*)res.bufs[0].buf, res.bufs[0].len);
        free(res.bufs[0].buf);
    }

    Zeus::close(qd);

    return 0;
}
