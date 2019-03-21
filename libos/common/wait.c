#include <dmtr/fail.h>
#include <dmtr/annot.h>
#include <dmtr/wait.h>
#include <dmtr/libos.h>
#include <errno.h>

int dmtr_wait(dmtr_qresult_t *qr_out, dmtr_qtoken_t qt) {
    int ret = EAGAIN;
    while (EAGAIN == ret) {
        ret = dmtr_poll(qr_out, qt);
    }
    DMTR_OK(dmtr_drop(qt));
    return ret;
}

int dmtr_wait_any(dmtr_qresult_t *qr_out, int *ready_offset, dmtr_qtoken_t qts[], int num_qts) {
    while (1) {
        for (int i = 0; i < num_qts; i++) {
            int ret = dmtr_poll(qr_out, qts[i]);
            if (ret != EAGAIN) {
                if (ret == 0)
                    DMTR_OK(dmtr_drop(qts[i]));
                if (ready_offset != NULL)
                    *ready_offset = i;
                return ret;
            }
        }
    }

    DMTR_UNREACHABLE();
}
