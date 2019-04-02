#include "common.hh"

#include <arpa/inet.h>
#include <boost/optional.hpp>
#include <cassert>
#include <cstring>
#include <dmtr/annot.h>
#include <dmtr/wait.h>
#include <iostream>
#include <libos/common/mem.h>
#include <netinet/in.h>
#include <unistd.h>
#include <signal.h>
#include <dmtr/libos.h>

int lqd = 0;
dmtr_timer_t *pop_timer = NULL;
dmtr_timer_t *push_timer = NULL;

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

    DMTR_OK(dmtr_init(argc, argv));
    DMTR_OK(dmtr_new_timer(&pop_timer, "pop server"));
    DMTR_OK(dmtr_new_timer(&push_timer, "push server"));

    std::vector<dmtr_qtoken_t> tokens;
    dmtr_qtoken_t token;
    DMTR_OK(dmtr_socket(&lqd, AF_INET, SOCK_STREAM, 0));
    std::cout << "listen qd: " << lqd << std::endl;

    DMTR_OK(dmtr_bind(lqd, reinterpret_cast<struct sockaddr *>(&saddr), sizeof(saddr)));


    DMTR_OK(dmtr_listen(lqd, 3));
    DMTR_OK(dmtr_accept(&token, lqd));
    tokens.push_back(token);
    
    if (signal(SIGINT, sig_handler) == SIG_ERR)
        std::cout << "\ncan't catch SIGINT\n";

    while (1) {
        dmtr_qresult wait_out;
        int idx;
        int status = dmtr_wait_any(&wait_out, &idx, tokens.data(), tokens.size());

        // if we got an EOK back from wait
        if (status == 0) {
	  //std::cout << "Found something: qd=" << wait_out.qr_qd;

            if (wait_out.qr_qd == lqd) {
                // check accept on servers
                DMTR_OK(dmtr_start_timer(pop_timer));
                DMTR_OK(dmtr_pop(&token, wait_out.qr_value.ares.qd));
                DMTR_OK(dmtr_stop_timer(pop_timer));
                tokens.push_back(token);
                DMTR_OK(dmtr_accept(&token, lqd));
                tokens[idx] = token;
            } else {
                assert(DMTR_OPC_POP == wait_out.qr_opcode);
                assert(wait_out.qr_value.sga.sga_numsegs == 1);
                //fprintf(stderr, "[%lu] server: rcvd\t%s\tbuf size:\t%d\n", i, reinterpret_cast<char *>(qr.qr_value.sga.sga_segs[0].sgaseg_buf), qr.qr_value.sga.sga_segs[0].sgaseg_len);
                DMTR_OK(dmtr_start_timer(push_timer));
                DMTR_OK(dmtr_push(&token, wait_out.qr_qd, &wait_out.qr_value.sga));
                DMTR_OK(dmtr_wait(NULL, token));
                DMTR_OK(dmtr_stop_timer(push_timer));
                DMTR_OK(dmtr_start_timer(pop_timer));
                DMTR_OK(dmtr_pop(&token, wait_out.qr_qd));
                DMTR_OK(dmtr_stop_timer(pop_timer));
                tokens[idx] = token;
                //fprintf(stderr, "send complete.\n");
                free(wait_out.qr_value.sga.sga_buf);
            }
        } else {
            assert(status == ECONNRESET || status == ECONNABORTED);
            dmtr_close(wait_out.qr_qd);
            tokens.erase(tokens.begin()+idx);
        }
    }
}


