// -*- mode: c++; c-file-style: "k&r"; c-basic-offset: 4 -*-
/***********************************************************************
 *
 * latency.h:
 *   latency profiling functions
 *
 * Copyright 2013-2015 Irene Zhang <iyzhang@cs.washington.edu>
 *                     Naveen Kr. Sharma <naveenks@cs.washington.edu>
 *                     Dan R. K. Ports  <drkp@cs.washington.edu>
 * Copyright 2009-2012 Massachusetts Institute of Technology
 *
 * Permission is hereby granted, free of charge, to any person
 * obtaining a copy of this software and associated documentation
 * files (the "Software"), to deal in the Software without
 * restriction, including without limitation the rights to use, copy,
 * modify, merge, publish, distribute, sublicense, and/or sell copies
 * of the Software, and to permit persons to whom the Software is
 * furnished to do so, subject to the following conditions:
 *
 * The above copyright notice and this permission notice shall be
 * included in all copies or substantial portions of the Software.
 *
 * THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND,
 * EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
 * MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND
 * NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS
 * BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN
 * ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN
 * CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
 * SOFTWARE.
 *
 **********************************************************************/

#ifndef _LIB_LATENCY_H_
#define _LIB_LATENCY_H_

//#include "lib/latency-format.pb.h"

#include <stdbool.h>
#include <stdint.h>
#include <time.h>
#include <iostream>
#include <fstream>
#include <vector>
#include <string>
#include <list>

// The number of the maximum distribution type.  Since we use
// characters as distribution types, this is 127.  We could probably
// shift things down by 32 to save some space, but that's probably not
// worth anything.
#define LATENCY_MAX_DIST 127

// The maximum number of unique distribution types in a single latency
// distribution.
#define LATENCY_DIST_POOL_SIZE 5

// The width of a printed histogram in characters.
#define LATENCY_HISTOGRAM_WIDTH 50

// The number of histogram buckets.
#define LATENCY_NUM_BUCKETS 65

// The maximum number of iterations we will record latencies for
#define MAX_ITERATIONS 1000000

typedef struct Latency_Frame_t
{
    uint64_t start;
    uint64_t accum;
    struct Latency_Frame_t *parent;
} Latency_Frame_t;

typedef struct Latency_Dist_t
{
    uint64_t min, max, total, count;
    uint32_t buckets[LATENCY_NUM_BUCKETS];
    char type;
} Latency_Dist_t;

typedef struct Latency_t
{
    std::string *name;

    Latency_Dist_t *dists[LATENCY_MAX_DIST];
    Latency_Dist_t distPool[LATENCY_DIST_POOL_SIZE];
    int distPoolNext = 0;

    Latency_Frame_t *bottom;
    Latency_Frame_t defaultFrame;

    std::vector<uint64_t> *latencies;
} Latency_t;

static std::list<Latency_t *> stats;

#define DEFINE_LATENCY(name)                                            \
    static Latency_t name;                                              \
    static __attribute__((constructor)) void _##name##_init(void)       \
    {                                                                   \
        _Latency_Init(&name, #name);                                    \
    }

void _Latency_Init(Latency_t *l, const char *name);

void Latency_StartRec(Latency_t *l, Latency_Frame_t *fr);
void Latency_EndRecType(Latency_t *l, Latency_Frame_t *fr, char type);
void Latency_Pause(Latency_t *l);
void Latency_Resume(Latency_t *l);

void Latency_Sum(Latency_t *dest, Latency_t *summand);

void Latency_Dump(Latency_t *l);
void Latency_DumpAll(void);

void init_time_resolution(void);

static inline void
Latency_Start(Latency_t *l)
{
    Latency_StartRec(l, &l->defaultFrame);
}

static inline void
Latency_EndRec(Latency_t *l, Latency_Frame_t *fr)
{
    Latency_EndRecType(l, fr, '=');
}

static inline void
Latency_EndType(Latency_t *l, char type)
{
    Latency_EndRecType(l, &l->defaultFrame, type);
}

static inline void
Latency_End(Latency_t *l)
{
    Latency_EndRec(l, &l->defaultFrame);
}

#endif // _LIB_LATENCY_H_
