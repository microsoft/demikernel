// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#include "common.hh"
#include <arpa/inet.h>
#include <boost/chrono.hpp>
#include <boost/optional.hpp>
#include <boost/program_options/options_description.hpp>
#include <boost/program_options/parsers.hpp>
#include <boost/program_options/variables_map.hpp>
#include <cstring>
#include <dmtr/annot.h>
#include <dmtr/latency.h>
#include <dmtr/libos.h>
#include <dmtr/libos/mem.h>
#include <dmtr/sga.h>
#include <dmtr/wait.h>
#include <iostream>
#include <netinet/in.h>
#include <unistd.h>
#include <yaml-cpp/yaml.h>
#include <sys/stat.h>
#include <sys/types.h>
#include <unistd.h>
#include <fcntl.h>

namespace po = boost::program_options;

int main(int argc, char *argv[])
{
    parse_args(argc, argv, false);

    DMTR_OK(dmtr_init(argc, argv));

    dmtr_latency_t *latency = NULL;
    DMTR_OK(dmtr_new_latency(&latency, "end-to-end"));

    int fqd = 0;
    assert(boost::none != file);
    // open a log file
    DMTR_OK(dmtr_open2(&fqd,  boost::get(file).c_str(), O_RDWR | O_CREAT | O_SYNC, S_IRWXU | S_IRGRP));
    printf("client qd:\t%d\n", fqd);

    dmtr_sgarray_t sga = {};
    sga.sga_numsegs = 1;
    sga.sga_segs[0].sgaseg_len = packet_size;
    sga.sga_segs[0].sgaseg_buf = generate_packet();

    for (size_t i = 0; i < iterations; i++) {
        dmtr_qtoken_t token;
        auto t0 = boost::chrono::steady_clock::now();
        DMTR_OK(dmtr_push(&token, fqd, &sga));
        DMTR_OK(dmtr_wait(NULL, token));
        auto log_dt = boost::chrono::steady_clock::now() - t0;
        DMTR_OK(dmtr_record_latency(latency, log_dt.count()));
            
        /*fprintf(stderr, "[%lu] client: rcvd\t%s\tbuf size:\t%d\n", i, reinterpret_cast<char *>(qr.qr_value.sga.sga_segs[0].sgaseg_buf), qr.qr_value.sga.sga_segs[0].sgaseg_len);*/
    }

    DMTR_OK(dmtr_dump_latency(stderr, latency));
    DMTR_OK(dmtr_close(fqd));

    return 0;
}
