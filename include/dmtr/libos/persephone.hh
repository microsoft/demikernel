#include <stdint.h>
#include <dmtr/annot.h>
#include <dmtr/fail.h>
#include <dmtr/sys/gcc.h>
#include <dmtr/libos/io_queue_api.hh>
#include <dmtr/libos.h>
#include <dmtr/libos/io_queue.hh>

class PspServiceUnit {
    public: dmtr::io_queue_api ioqapi;
    uint32_t my_id;
    dmtr::io_queue::category_id my_type;

    public: PspServiceUnit(uint32_t id,
                           dmtr::io_queue::category_id type,
                           int argc, char *argv[]): my_id(id), my_type(type) {
        init(argc, argv);
    };

    public: int wait(dmtr_qresult_t *qr_out, dmtr_qtoken_t qt);
    public: int wait_any(dmtr_qresult_t *qr_out, int *start_offset, int *ready_offset, dmtr_qtoken_t qts[], int num_qts);

    private: DMTR_EXPORT int init(int argc, char *argv[]);

    // delete copy ctor
};
