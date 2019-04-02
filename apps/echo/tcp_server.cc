#include "common.hh"

#include <arpa/inet.h>
#include <boost/optional.hpp>
#include <cassert>
#include <cstring>
#include <dmtr/annot.h>
#include <dmtr/libos.h>
#include <dmtr/wait.h>
#include <iostream>
#include <libos/common/mem.h>
#include <netinet/in.h>
#include <unistd.h>
#include <signal.h>

int lqd = 0;
dmtr_timer_t *pop_timer = NULL;
dmtr_timer_t *push_timer = NULL;

/* Will dump the timers when Ctrl-C to close server 
*  Since we MUST exit, not return after this function, cannot use DMTR_OK here
*/
void sig_handler(int signo)
{
    std::cout << std::endl;
    if (NULL != pop_timer && NULL != push_timer) {
        dmtr_dump_timer(stderr, pop_timer);
        dmtr_dump_timer(stderr, push_timer);
    }

    dmtr_close(lqd);
    exit(0);
}

/* Server that loops for multiple clients of arbitrary iterations */
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

    DMTR_OK(dmtr_socket(&lqd, AF_INET, SOCK_STREAM, 0));
 
    DMTR_OK(dmtr_bind(lqd, reinterpret_cast<struct sockaddr *>(&saddr), sizeof(saddr)));
    DMTR_OK(dmtr_listen(lqd, 3));

    std::vector<dmtr_qtoken_t> qts;
    dmtr_qtoken_t qt = 0;
    DMTR_OK(dmtr_accept(&qt, lqd));
    qts.push_back(qt);

    if(signal(SIGINT, sig_handler) == SIG_ERR) {
        std::cout << "\nWARNING: can't catch SIGINT" << std::endl;
    }

    while(1) {
        dmtr_qresult_t qr = {};
        int idx;
        int status = dmtr_wait_any(&qr, &idx, qts.data(), qts.size());

        if(0 == status) { // EOK
            // Task was to accept a new connection
            if (qr.qr_qd == lqd) {
                DMTR_TRUE(EPERM, DMTR_OPC_ACCEPT == qr.qr_opcode);

                // put new qtoken in for new connection
                int new_qd = qr.qr_value.ares.qd;
                dmtr_qtoken_t new_qt = 0;

                DMTR_OK(dmtr_start_timer(pop_timer));
                DMTR_OK(dmtr_pop(&new_qt, new_qd));
                DMTR_OK(dmtr_stop_timer(pop_timer));
                qts.push_back(new_qt);

                DMTR_OK(dmtr_accept(&qt, lqd));
                qts[idx] = qt;
            }
            // Task was a read complete 
            else {
                DMTR_TRUE(EPERM, DMTR_OPC_POP == qr.qr_opcode);
                DMTR_TRUE(EPERM, qr.qr_value.sga.sga_numsegs == 1);

                DMTR_OK(dmtr_start_timer(push_timer));
                DMTR_OK(dmtr_push(&qt, qr.qr_qd, &qr.qr_value.sga));
                DMTR_OK(dmtr_wait(NULL, qt));
                DMTR_OK(dmtr_stop_timer(push_timer));

                DMTR_OK(dmtr_start_timer(pop_timer));
                DMTR_OK(dmtr_pop(&qt, qr.qr_qd));
                DMTR_OK(dmtr_stop_timer(pop_timer));
                qts[idx] = qt;

                free(qr.qr_value.sga.sga_buf);
            }
        }
        else if (ECONNABORTED == status || ECONNRESET == status) {
            DMTR_OK(dmtr_close(qr.qr_qd));
            qts.erase(qts.begin()+idx);
        }
        else {
            DMTR_UNREACHABLE();
        }
    } // end while(1)
}
