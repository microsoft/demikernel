#include <dmtr/annot.h>
#include <dmtr/libos.h>
#include <dmtr/mem.h>
#include <dmtr/wait.h>
#include <libos/common/latency.h>

#include <netinet/in.h>
#include <assert.h>
#include <arpa/inet.h>

#include <cassert>
#include <cstring>
#include <unistd.h>
#include "common.hh"
#include <vector>

int main(int argc, char **argv)
{

    parse_args(argc, argv);
    DMTR_OK(dmtr_init(dmtr_argc, dmtr_argv));

    int lqd = 0;
    DMTR_OK(dmtr_socket(&lqd, AF_INET, SOCK_STREAM, 0));
    //printf("listen qd:\t%d\n", lqd);

    struct sockaddr_in saddr = {};
    saddr.sin_family = AF_INET;
    saddr.sin_addr.s_addr = htonl(INADDR_ANY);
 //   if (inet_pton(AF_INET, ip.c_str(), &saddr.sin_addr) != 1) {
 //       printf("Address not supported!\n");
 //     return -1;
 //   }
    // todo: this should be done from within the libos.
    saddr.sin_port = htons(port);
    DMTR_OK(dmtr_bind(lqd, reinterpret_cast<struct sockaddr *>(&saddr), sizeof(saddr)));

    //printf("listening for connections\n");
    DMTR_OK(dmtr_listen(lqd, 3));

    std::vector<dmtr_qtoken_t> qts;
    dmtr_qresult_t qr = {};

    dmtr_qtoken_t accept_qt = 0;
    DMTR_OK(dmtr_accept(&accept_qt, lqd));
    qts.push_back(accept_qt);
    while (1) {
        int status = dmtr_wait_any(&qr, qts.data(), qts.size());

        // take the used q_token out
        for (auto qt = qts.begin(); qt != qts.end(); qt++) {
            if (*qt == qr.qr_qt) {
                qts.erase(qt);
                break;
            }
        }
        
        if (qr.qr_qd == lqd) {
            DMTR_TRUE(EPERM, DMTR_OPC_ACCEPT == qr.qr_opcode);
            DMTR_TRUE(EPERM, DMTR_TID_QD == qr.qr_tid);

            // put new qtoken in for new connection
            int new_qd = qr.qr_value.qd;
            dmtr_qtoken_t new_qt = 0;
            DMTR_OK(dmtr_pop(&new_qt, new_qd));
            qts.push_back(new_qt);

            // replace used qtoken
            DMTR_OK(dmtr_accept(&accept_qt, lqd));
            qts.push_back(accept_qt);
        } else {
            switch (status) {
            case 0: // EOK
                {
                    DMTR_TRUE(EPERM, DMTR_OPC_POP == qr.qr_opcode);
                    DMTR_TRUE(EPERM, DMTR_TID_SGA == qr.qr_tid);
                    DMTR_TRUE(EPERM, qr.qr_value.sga.sga_numsegs == 1);
                    
                    //fprintf(stderr, "[%lu] server: rcvd\t%s\tbuf size:\t%d\n",
                    //i, reinterpret_cast<char *>(qr.qr_value.sga.sga_segs[0].sgaseg_buf),
                    //qr.qr_value.sga.sga_segs[0].sgaseg_len);

                    // push received data back into queue
                    int push_qd = qr.qr_qd;
                    dmtr_qtoken_t push_qt = 0;
                    DMTR_OK(dmtr_push(&push_qt, push_qd, &qr.qr_value.sga));
                    DMTR_OK(dmtr_wait(NULL, push_qt));
                    //fprintf(stderr, "send complete.\n");
                    free(qr.qr_value.sga.sga_buf);

                    // replace used qtoken
                    dmtr_qtoken_t new_qt = 0;
                    DMTR_OK(dmtr_pop(&new_qt, push_qd));
                    qts.push_back(new_qt);
                    break;
                }
            case ECONNRESET:
            case ECONNABORTED:
                {
                    // connection closed
                    DMTR_OK(dmtr_close(qr.qr_qd));
                    fprintf(stderr, "Connection closed\n");
                    break;
                }
            default:
                DMTR_UNREACHABLE();
            }
            
        }
    }
    DMTR_OK(dmtr_close(lqd));

    return 0;
}
