#include <dmtr/wait.h>

#include <dmtr/libos.h>

#include <errno.h>

int dmtr_wait(dmtr_qresult_t *qr_out, dmtr_qtoken_t qt) {
    int ret = EAGAIN;
    while (EAGAIN == ret) {
        ret = dmtr_poll(qr_out, qt);
    }

    return ret;
}
