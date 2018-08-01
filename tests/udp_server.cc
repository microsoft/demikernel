#include <iostream>
#include <string.h>
#include <netinet/in.h>
#include <assert.h>
#include <arpa/inet.h>

#include "../include/io-queue.h"

uint16_t port = 12345;

int main()
{
    int qd;
    ssize_t n;
    Zeus::qtoken qt;
    struct Zeus::sgarray sga;
    struct sockaddr_in server;

    char* argv[] = {(char*)"",
                    (char*)"-c",
                    (char*)"0x1",
                    (char*)"-n",
                    (char*)"4",
                    (char*)"--proc-type=auto",
                    (char*)""};
    int argc = 6;

    if (Zeus::init(argc, argv) < 0) {
        printf("Error initializing Zeus!\n");
        return -1;
    }

    if ((qd = Zeus::socket(AF_INET, SOCK_DGRAM, 0)) < 0) {
        printf("Error creating queue!\n");
        return -1;
    }

    printf("server qd:\t%d\n", qd);


    server.sin_family = AF_INET;
    if (inet_pton(AF_INET, "10.0.0.5", &(server.sin_addr)) != 1) {
        printf("Address not supported!\n");
        return -1;
    }
    server.sin_port = htons(port);

    if (Zeus::bind(qd, (struct sockaddr*)&server, sizeof(server)) < 0) {
        printf("Error binding queue!\n");
        return -1;
    }
    
//    while (1) {
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
//    }

    Zeus::close(qd);

    return 0;
}
