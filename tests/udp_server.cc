#include <iostream>
#include <string.h>
#include <netinet/in.h>
#include <assert.h>

#include "../include/io-queue.h"

uint16_t port = 12345;

int main()
{
    int qd;
    ssize_t n;
    Zeus::qtoken qt;
    struct Zeus::sgarray sga;
    char buf[18] = "hello from server";
    struct sockaddr_in server;

    char* argv[] = {(char*)"",
                    (char*)"-b",
                    (char*)"0000:03:00.1",
                    (char*)"-l",
                    (char*)"1",
                    (char*)"-m",
                    (char*)"256",
                    (char*)"--no-shconf",
                    (char*)"--file-prefix",
                    (char*)"s" };
    int argc = 10;

    if (Zeus::init(argc, argv) < 0) {
        printf("Error initializing Zeus!\n");
        return -1;
    }

    if ((qd = Zeus::queue(AF_INET, SOCK_DGRAM, 0)) < 0) {
        printf("Error creating queue!\n");
        return -1;
    }

    printf("server qd:\t%d\n", qd);

    server.sin_family = AF_INET;
    server.sin_addr.s_addr = htonl(0x0c0c0c04);
    server.sin_port = htons(port);

    if (Zeus::bind(qd, (struct sockaddr*)&server, sizeof(server)) < 0) {
        printf("Error binding queue!\n");
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

	sga.num_bufs = 1;
	sga.bufs[0].len = 18;
	sga.bufs[0].buf = (Zeus::ioptr)buf;

	qt = Zeus::push(qd, sga);
	if (qt != 0) {
		printf("server: wait for push\n");
		n = Zeus::wait(qt, sga);
		assert(n > 0);
	}

	printf("server sent:\t%s\n", (char*)sga.bufs[0].buf);

    Zeus::close(qd);

    return 0;
}
