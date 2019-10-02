// -*- mode: c++; c-file-style: "k&r"; c-basic-offset: 4 -*-

// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#include <stdio.h>

#include <dmtr/trace.hh>
#include <dmtr/annot.h>
#include <dmtr/latency.h> //FIXME: for the log_directory

// The maximum number of entries we will record in a trace
#define MAX_TRACES 10000000

int dmtr_new_trace(dmtr_trace_t **trace_out, const char *name) {
    DMTR_NOTNULL(EINVAL, trace_out);
    *trace_out = NULL;
    DMTR_NOTNULL(EINVAL, name);

    auto trace = new dmtr_trace_t();
    trace->name = name;
    trace->traces.reserve(MAX_TRACES);

    *trace_out = trace;
    return 0;
}

int dmtr_record_trace(dmtr_trace_t *trace, dmtr_qtoken_trace_t &qt_trace) {
    DMTR_NOTNULL(EINVAL, trace);
    DMTR_NONZERO(EINVAL, qt_trace.token);

    if (trace->traces.size() < MAX_TRACES) {
        trace->traces.push_back(qt_trace);
    }

    return 0;
}

int dmtr_dump_trace_to_file(const char *filename, dmtr_trace_t *trace) {
    DMTR_NOTNULL(EINVAL, trace);
    FILE *f = fopen(filename, "w");
    DMTR_NOTNULL(EINVAL, f);
    fprintf(f, "TOKEN\tSTART\tTIME\n");
    for (auto &t: trace->traces) {
        fprintf(f, "%lu\t%s\t%lu\n", t.token, t.start ? "true" : "false", since_epoch(t.timestamp));
    }
    DMTR_OK(fclose(f));
    return 0;
}

int dmtr_delete_trace(dmtr_trace_t **trace) {
    DMTR_NOTNULL(EINVAL, trace);
    delete *trace;
    *trace = NULL;
    return 0;
}

int dmtr_register_trace(const char *label,
                        std::unordered_map<pthread_t, trace_ptr_type> &traces) {
    char log_filename[MAX_LOG_FILENAME_LEN];
    const char *log_dir = dmtr_log_directory.c_str();
    pthread_t me = pthread_self();

    auto it = traces.find(me);
    DMTR_TRUE(EINVAL, it == traces.end());
    snprintf(log_filename, MAX_LOG_FILENAME_LEN, "%s/%ld-%s-traces", log_dir, me, label);
    dmtr_trace_t *t;
    DMTR_OK(dmtr_new_trace(&t, label));
    trace_ptr_type trace =
        trace_ptr_type(t, [log_filename](dmtr_trace_t *trc) {
            dmtr_dump_trace_to_file(reinterpret_cast<const char *>(log_filename), trc);
            dmtr_delete_trace(&trc);
        });
    traces.insert(
        std::pair<pthread_t, trace_ptr_type>(me, std::move(trace))
    );

    return 0;
}
