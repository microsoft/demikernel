// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#ifndef DMTR_TRACE_H_IS_INCLUDED
#define DMTR_TRACE_H_IS_INCLUDED

#include <functional>
#include <vector>
#include <memory>
#include <unordered_map>

#include <dmtr/time.hh>
#include <dmtr/types.h>

typedef struct dmtr_qtoken_trace {
    dmtr_qtoken_t token;
    bool start; /** < Is this the beginning of the operation */
    tp timestamp;
} dmtr_qtoken_trace_t;

typedef struct dmtr_trace {
    std::string name;
    std::vector<dmtr_qtoken_trace_t> traces; //FIXME those should hold references to avoid copy
} dmtr_trace_t;

typedef std::unique_ptr<dmtr_trace_t, std::function<void(dmtr_trace_t *)> > trace_ptr_type;

int dmtr_dump_trace_to_file(const char * filename, dmtr_trace_t *trace);
int dmtr_record_trace(dmtr_trace_t &trace, dmtr_qtoken_trace_t &qt_trace);
int dmtr_new_trace(dmtr_trace_t **trace_out, const char *name);
int dmtr_delete_trace(dmtr_trace_t **trace);
int dmtr_register_trace(const char *label, std::string log_dir, trace_ptr_type &traces);

#endif /* DMTR_TRACE_H_IS_INCLUDED */
