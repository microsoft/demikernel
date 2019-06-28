// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#include <dmtr/wait.h>

#include <boost/chrono.hpp>
#include <cerrno>
#include <dmtr/annot.h>
#include <dmtr/fail.h>
#include <dmtr/latency.h>
#include <dmtr/libos.h>
#include <pthread.h>
#include <unordered_map>

#define DMTR_PROFILE 1

#if DMTR_PROFILE
typedef std::unique_ptr<dmtr_latency_t, std::function<void(dmtr_latency_t *)>> latency_ptr_type;
static std::unordered_map<pthread_t, latency_ptr_type> success_poll_latencies;
#endif

int dmtr_wait(dmtr_qresult_t *qr_out, dmtr_qtoken_t qt) {
    int ret = EAGAIN;
    while (EAGAIN == ret) {
        ret = dmtr_poll(qr_out, qt);
    }
    DMTR_OK(dmtr_drop(qt));
    return ret;
}

int dmtr_wait_any(dmtr_qresult_t *qr_out, int *ready_offset, dmtr_qtoken_t qts[], int num_qts) {
#if DMTR_PROFILE
    pthread_t me = pthread_self();
    dmtr_latency_t *l;
    auto it = success_poll_latencies.find(me);
    if (it == success_poll_latencies.end()) {
        DMTR_OK(dmtr_new_latency(&l, "dmtr success poll"));
        latency_ptr_type success_poll_latency = latency_ptr_type(l, [](dmtr_latency_t *latency) {
            dmtr_dump_latency(stderr, latency);
            dmtr_delete_latency(&latency);
        });
        success_poll_latencies.insert(
            std::pair<pthread_t, latency_ptr_type>(me, std::move(success_poll_latency))
        );
    } else {
        l = it->second.get();
    }
#endif
    while (1) {
        for (int i = 0; i < num_qts; i++) {
#if DMTR_PROFILE
            auto t0 = boost::chrono::steady_clock::now();
#endif
            int ret = dmtr_poll(qr_out, qts[i]);
            if (ret != EAGAIN) {
                if (ret == 0 || ret == ECONNABORTED) {
                    DMTR_OK(dmtr_drop(qts[i]));
#if DMTR_PROFILE
                    auto dt = (boost::chrono::steady_clock::now() - t0);
                    DMTR_OK(dmtr_record_latency(l, dt.count()));
#endif
                    if (ready_offset != NULL)
                        *ready_offset = i;
                    return ret;
                }
            }
        }
    }

    DMTR_UNREACHABLE();
}
