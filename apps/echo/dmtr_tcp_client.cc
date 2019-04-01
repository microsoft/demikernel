#include "common.hh"
#include <dmtr/annot.h>
#include <dmtr/libos.h>
#include <libos/common/mem.h>
#include <dmtr/wait.h>

#include <arpa/inet.h>
#include <boost/optional.hpp>
#include <boost/program_options/options_description.hpp>
#include <boost/program_options/parsers.hpp>
#include <boost/program_options/variables_map.hpp>
#include <cstring>
#include <iostream>
#include <netinet/in.h>
#include <unistd.h>
#include <yaml-cpp/yaml.h>

#define ITERATION_COUNT 10000
#define BUFFER_SIZE 1024
#define FILL_CHAR 'a'

namespace po = boost::program_options;

int main(int argc, char *argv[])
{
    parse_args(argc, argv, false);
        struct sockaddr_in saddr = {};
    saddr.sin_family = AF_INET;
    std::cerr << "Listening on `*:" << port << "`..." << std::endl;
    saddr.sin_addr.s_addr = INADDR_ANY;
    saddr.sin_port = htons(port);

    DMTR_OK(dmtr_init(argc, argv));

    dmtr_timer_t *timer = NULL;
    DMTR_OK(dmtr_new_timer(&timer, "end-to-end"));

    int qd = 0;
    DMTR_OK(dmtr_socket(&qd, AF_INET, SOCK_STREAM, 0));
    printf("client qd:\t%d\n", qd);

    std::cerr << "Attempting to connect to `" << boost::get(server_ip_addr) << ":" << port << "`..." << std::endl;
    DMTR_OK(dmtr_connect(qd, reinterpret_cast<struct sockaddr *>(&saddr), sizeof(saddr)));
    std::cerr << "Connected." << std::endl;

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
        DMTR_OK(dmtr_start_timer(timer));
        DMTR_OK(dmtr_push(&qt, qd, &sga));
        DMTR_OK(dmtr_wait(NULL, qt));
        //fprintf(stderr, "send complete.\n");

        dmtr_qresult_t qr = {};
        DMTR_OK(dmtr_pop(&qt, qd));
        DMTR_OK(dmtr_wait(&qr, qt));
        DMTR_OK(dmtr_stop_timer(timer));
        assert(DMTR_OPC_POP == qr.qr_opcode);
        assert(qr.qr_value.sga.sga_numsegs == 1);
        assert(reinterpret_cast<uint8_t *>(qr.qr_value.sga.sga_segs[0].sgaseg_buf)[0] == FILL_CHAR);

        /*fprintf(stderr, "[%lu] client: rcvd\t%s\tbuf size:\t%d\n", i, reinterpret_cast<char *>(qr.qr_value.sga.sga_segs[0].sgaseg_buf), qr.qr_value.sga.sga_segs[0].sgaseg_len);*/
        free(qr.qr_value.sga.sga_buf);
    }
    free(p);
    DMTR_OK(dmtr_dump_timer(stderr, timer));
    DMTR_OK(dmtr_close(qd));

    return 0;
}
