#ifndef PERSEPHONE_H_
#define PERSEPHONE_H_

#include <yaml-cpp/yaml.h>
#include <dmtr/libos/io/io_queue_api.hh>
#include <dmtr/libos.h>

/*************** Gloval variables **************/
// Logging
static std::string log_dir;
#define MAX_LOG_FILENAME_LEN 128

/************** Service Units ************/
class PspServiceUnit {
    /* IO related variables and functions */
    public: uint32_t my_id;
    private: struct io_context { /* << The various contexts used by io queues */
        void *net_context; /* << The context used to setup network queues */
    };
    public: struct io_context io_ctx;
    public: bool net_context_init_flag = false;
    public: dmtr::io_queue_api ioqapi;

    public: PspServiceUnit(uint32_t id) : my_id(id) {
        io_ctx.net_context = NULL;
        dmtr_init_ctors(static_cast<void *>(&ioqapi));
    };

    public: int wait(dmtr_qresult_t *qr_out, dmtr_qtoken_t qt);
    public: int wait_any(dmtr_qresult_t *qr_out, int *start_offset, int *ready_offset, dmtr_qtoken_t qts[], int num_qts);

    //public: shared_queue(int shared_qfd, dmtr::shared_item *consumer_si, dmtr::shared_item *producer_si);
    public: int socket(int &qd, int domain, int type, int protocol);

    //Convenient way to store which ip:port to connect/bind to
    public: std::string ip;
    public: uint16_t port;

    public: ~PspServiceUnit() {
        if (io_ctx.net_context != NULL) { //if (net_context_init_flag) ?
            dmtr_del_net_context(io_ctx.net_context);
        }
    }
};

/************** Control Plane class ************/
class Psp {
    public: Psp(std::string &app_cfg);

    private: struct net_context {
        void * net_mempool;
    };
    private: struct net_context net_ctx; /* << The global network context */
    public: std::unordered_map<uint32_t, std::shared_ptr<PspServiceUnit> > service_units;
};

#endif // PERSEPHONE_H_
