#include <arpa/inet.h>
#include <boost/optional.hpp>
#include <boost/program_options/options_description.hpp>
#include <boost/program_options/parsers.hpp>
#include <boost/program_options/variables_map.hpp>
#include <cassert>
#include <cstring>
#include <dmtr/annot.h>
#include <dmtr/libos.h>
#include <dmtr/wait.h>
#include <iostream>
#include <libos/common/mem.h>
#include <netinet/in.h>
#include <yaml-cpp/yaml.h>
#include <signal.h>

#include "common.hh"

int lqd = 0;
dmtr_timer_t *pop_timer = NULL;
dmtr_timer_t *push_timer = NULL;
namespace po = boost::program_options;

void sig_handler(int signo)
{
  dmtr_dump_timer(stderr, pop_timer);
  dmtr_dump_timer(stderr, push_timer);
  dmtr_close(lqd);
  exit(0);
}

int main(int argc, char *argv[])
{
    parse_args(argc, argv, true);
    
    struct sockaddr_in saddr = {};
    saddr.sin_family = AF_INET;
    if (boost::none == server_ip_addr) {
        std::cerr << "Listening on `*:" << port << "`..." << std::endl;
        saddr.sin_addr.s_addr = INADDR_ANY;
    } else {
        const char *s = boost::get(server_ip_addr).c_str();
        std::cerr << "Listening on `" << s << ":" << port << "`..." << std::endl;
        if (inet_pton(AF_INET, s, &saddr.sin_addr) != 1) {
            std::cerr << "Unable to parse IP address." << std::endl;
            return -1;
        }
    }
    saddr.sin_port = htons(port);

    DMTR_OK(dmtr_init(dmtr_argc, dmtr_argv));

    DMTR_OK(dmtr_new_timer(&pop_timer, "pop"));
    DMTR_OK(dmtr_new_timer(&push_timer, "push"));

    DMTR_OK(dmtr_socket(&lqd, AF_INET, SOCK_DGRAM, 0));
    printf("server qd:\t%d\n", lqd);

    DMTR_OK(dmtr_bind(lqd, reinterpret_cast<struct sockaddr *>(&saddr), sizeof(saddr)));

    if (signal(SIGINT, sig_handler) == SIG_ERR)
        std::cout << "\ncan't catch SIGINT\n";

    while(1) {
        dmtr_qresult_t qr = {};
        dmtr_qtoken_t qt = 0;
        DMTR_OK(dmtr_start_timer(pop_timer));
        DMTR_OK(dmtr_pop(&qt, lqd));
        DMTR_OK(dmtr_wait(&qr, qt));
        DMTR_OK(dmtr_stop_timer(pop_timer));
        assert(DMTR_OPC_POP == qr.qr_opcode);
        assert(qr.qr_value.sga.sga_numsegs == 1);

        //fprintf(stderr, "[%lu] server: rcvd\t%s\tbuf size:\t%d\n", i, reinterpret_cast<char *>(qr.qr_value.sga.sga_segs[0].sgaseg_buf), qr.qr_value.sga.sga_segs[0].sgaseg_len);
        DMTR_OK(dmtr_start_timer(push_timer));
        DMTR_OK(dmtr_push(&qt, lqd, &qr.qr_value.sga));
        DMTR_OK(dmtr_wait(NULL, qt));
        DMTR_OK(dmtr_stop_timer(push_timer));

        //fprintf(stderr, "send complete.\n");
        free(qr.qr_value.sga.sga_buf);
    }

    return 0;
}
