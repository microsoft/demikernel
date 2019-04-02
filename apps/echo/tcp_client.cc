#include "common.hh"

#include <arpa/inet.h>
#include <cstring>
#include <dmtr/annot.h>
#include <dmtr/libos.h>
#include <dmtr/wait.h>
#include <iostream>
#include <libos/common/mem.h>
#include <netinet/in.h>
#include <unistd.h>

int main(int argc, char *argv[])
{
    parse_args(argc, argv, false);

    if (boost::none == server_ip_addr) {
        std::cerr << "Server IP address is required" << std::endl;
        return -1;
    }

    struct sockaddr_in saddr = {};
    saddr.sin_family = AF_INET;
    saddr.sin_port = htons(port);
    if (inet_pton(AF_INET, server_ip_addr->c_str(), &saddr.sin_addr) != 1) {
        std::cerr << "Unable to parse IP address." << std::endl;
        return -1;
    }

    DMTR_OK(dmtr_init(dmtr_argc, dmtr_argv));

    dmtr_timer_t *pop_timer = NULL;
    DMTR_OK(dmtr_new_timer(&pop_timer, "pop"));
    dmtr_timer_t *push_timer = NULL;
    DMTR_OK(dmtr_new_timer(&push_timer, "push"));

    int qd = 0;
    DMTR_OK(dmtr_socket(&qd, AF_INET, SOCK_STREAM, 0));
    printf("client qd:\t%d\n", qd);

    std::cerr << "Attempting to connect to `" << *server_ip_addr << ":" << port << "`..." << std::endl;
    DMTR_OK(dmtr_connect(qd, reinterpret_cast<struct sockaddr *>(&saddr), sizeof(saddr)));
    std::cerr << "Connected." << std::endl;

    // Use the generate_packet() utility from common.hh
    dmtr_sgarray_t sga = {};
    sga.sga_numsegs = 1;
    sga.sga_segs[0].sgaseg_len = packet_size;
    sga.sga_segs[0].sgaseg_buf = generate_packet();

    for (size_t i = 0; i < iterations; i++) {
        dmtr_qtoken_t qt;
        DMTR_OK(dmtr_start_timer(push_timer));
        DMTR_OK(dmtr_push(&qt, qd, &sga));
        DMTR_OK(dmtr_wait(NULL, qt));
        DMTR_OK(dmtr_stop_timer(push_timer));
        //fprintf(stderr, "send complete.\n");

        dmtr_qresult_t qr = {};
        DMTR_OK(dmtr_start_timer(pop_timer));
        DMTR_OK(dmtr_pop(&qt, qd));
        DMTR_OK(dmtr_wait(&qr, qt));
        DMTR_OK(dmtr_stop_timer(pop_timer));
        DMTR_TRUE(EPERM, DMTR_OPC_POP == qr.qr_opcode);
        DMTR_TRUE(EPERM, qr.qr_value.sga.sga_numsegs == 1);
        DMTR_TRUE(EPERM, reinterpret_cast<uint8_t *>(qr.qr_value.sga.sga_segs[0].sgaseg_buf)[0] == FILL_CHAR);

        /*fprintf(stderr, "[%lu] client: rcvd\t%s\tbuf size:\t%d\n", i, reinterpret_cast<char *>(qr.qr_value.sga.sga_segs[0].sgaseg_buf), qr.qr_value.sga.sga_segs[0].sgaseg_len);*/
        free(qr.qr_value.sga.sga_buf);
    }

    DMTR_OK(dmtr_dump_timer(stderr, pop_timer));
    DMTR_OK(dmtr_dump_timer(stderr, push_timer));
    DMTR_OK(dmtr_close(qd));

    return 0;
}
