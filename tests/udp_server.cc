#include <iostream>
#include <string.h>
#include <netinet/in.h>
#include <assert.h>
#include <arpa/inet.h>

#include "../include/io-queue.h"
#include "../libos/common/latency.h"

#define PKTNUM          10000

uint16_t port = 12345;

int main()
{
    int qd;
    ssize_t n;
    Zeus::qtoken qt;
    struct Zeus::sgarray sga;
    struct sockaddr_in server;
/*
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
*/
    if (Zeus::init() < 0) {
        printf("Error initializing Zeus!\n");
        return -1;
    }

    if ((qd = Zeus::socket(AF_INET, SOCK_DGRAM, 0)) < 0) {
        printf("Error creating queue!\n");
        return -1;
    }

    printf("server qd:\t%d\n", qd);


    server.sin_family = AF_INET;
    if (inet_pton(AF_INET, "12.12.12.4", &(server.sin_addr)) != 1) {
        printf("Address not supported!\n");
        return -1;
    }
    server.sin_port = htons(port);

    if (Zeus::bind(qd, (struct sockaddr*)&server, sizeof(server)) < 0) {
        printf("Error binding queue!\n");
        return -1;
    }
    
    for (int i = 0; i < PKTNUM; i++) {
//		qt = Zeus::pop(qd, sga);
//		if (qt != 0) {
//			//printf("server: wait for pop\n");
//			n = Zeus::wait(qt, sga);
//			assert(n > 0);
//		}

        while(Zeus::peek(qd, sga) == 0);

		assert(sga.num_bufs == 1);

		//printf("server rcvd:\t%s\n", (char*)sga.bufs[0].buf);

		qt = Zeus::push(qd, sga);
		if (qt != 0) {
			//printf("server: wait for push\n");
			n = Zeus::wait(qt, sga);
			assert(n > 0);
		}

		//printf("===========================\n");i
		//printf("server sent:\t%s\n", (char*)sga.bufs[0].buf);
        free(sga.bufs[0].buf);
    }

    Latency_DumpAll();
    Zeus::close(qd);

    return 0;
}
