#include <dmtr/wait.h>

#include <dmtr/libos.h>

#include <errno.h>

int dmtr_wait(dmtr_sgarray_t * const sga_out, dmtr_qtoken_t qt) {
    int ret = EAGAIN;
    while (EAGAIN == ret) {
        ret = dmtr_poll(sga_out, qt);
    }

    return ret;
}
