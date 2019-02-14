#include <dmtr/fail.h>
#include <dmtr/annot.h>
#include <dmtr/wait.h>
#include <dmtr/libos.h>
#include <errno.h>

int dmtr_wait(dmtr_sgarray_t * const sga_out, dmtr_qtoken_t qt) {
    int ret = EAGAIN;
    while (EAGAIN == ret) {
        ret = dmtr_poll(sga_out, qt);
    }
    DMTR_OK(dmtr_drop(qt));
    return ret;
}

int dmtr_wait_any(dmtr_wait_completion_t * const wait_out, dmtr_qtoken_t qts[], int num_qts) {
    while (1) {
        for (int i = 0; i < num_qts; i++) {
            int ret = dmtr_poll(&wait_out->sga_out, qts[i]);
            if (ret != EAGAIN) {
                wait_out->qt_out = qts[i];
                wait_out->qt_idx_out = i;
                wait_out->qd_out = QT2QD(qts[i]);
                DMTR_OK(dmtr_drop(qts[i]));
                return ret;
            }
        }
    }
        
    DMTR_UNREACHABLE();
}
