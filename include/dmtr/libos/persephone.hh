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
    struct io_context { /* << The various contexts used by io queues */
        void *net_context; /* << The context used to setup network queues */
    };
    public: struct io_context *io_ctx;
    public: dmtr::io_queue_api ioqapi;
    uint32_t my_id;
    dmtr::io_queue::category_id my_type;

    public: PspServiceUnit(uint32_t id) : my_id(id) {
        dmtr_init_ctors(static_cast<void *>(&ioqapi));
    };

    public: int wait(dmtr_qresult_t *qr_out, dmtr_qtoken_t qt);
    public: int wait_any(dmtr_qresult_t *qr_out, int *start_offset, int *ready_offset, dmtr_qtoken_t qts[], int num_qts);

    //public: shared_queue(int shared_qfd, dmtr::shared_item *consumer_si, dmtr::shared_item *producer_si);
    public: int socket(int &qd, int domain, int type, int protocol);

    private: DMTR_EXPORT int init(int argc, char *argv[]);

    // delete copy ctor
    /*
    public: ~PspServiceUnit() {
        delete io_ctx->net_context; //FIXME this should be a call to smth like dmtr_del_net_context(void *context)
    }
    */
};

/************** Control Plane class ************/
class Psp {
    public: Psp(std::string &app_cfg);

    private: struct net_context {
        void * net_mempool;
        void * frag_ctx;
    };
    private: struct net_context net_ctx; /* << The global network context */
    public: std::vector<std::shared_ptr<PspServiceUnit> > service_units;
};

#endif // PERSEPHONE_H_
