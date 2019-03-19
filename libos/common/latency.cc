// -*- mode: c++; c-file-style: "k&r"; c-basic-offset: 4 -*-
/***********************************************************************
 *
 * latency.cc:
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

#include <dmtr/libos.h>

#include <algorithm>
#include <cassert>
#include <dmtr/annot.h>
#include <dmtr/fail.h>
#include <plf_nanotimer.h>
#include <stdint.h>
#include <string>
#include <vector>

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

typedef struct Latency_Dist_t
{
    uint64_t min, max, total, count;
    uint32_t buckets[LATENCY_NUM_BUCKETS];
    char type;
} Latency_Dist_t;

typedef struct dmtr_timer
{
    std::string name;
    plf::nanotimer timer;

    Latency_Dist_t *dists[LATENCY_MAX_DIST];
    Latency_Dist_t distPool[LATENCY_DIST_POOL_SIZE];
    int distPoolNext = 0;

    std::vector<uint64_t> latencies;
} dmtr_timer_t;

static inline void
LatencyAddStat(dmtr_timer_t *l, char type, uint64_t val)
{
    //if (l->latencies.size() == 0)

    if (l->latencies.size() < MAX_ITERATIONS)
	l->latencies.push_back(val);
}


static inline Latency_Dist_t *
LatencyAddHist(dmtr_timer_t *l, char type, uint64_t val, uint32_t count)
{
    if (!l->dists[(int)type]) {
        if (l->distPoolNext == LATENCY_DIST_POOL_SIZE) {
            DMTR_PANIC("Too many distributions; maybe increase "
                  "LATENCY_DIST_POOL_SIZE");
        }
        l->dists[(int)type] = &l->distPool[l->distPoolNext++];
        l->dists[(int)type]->type = type;
    }
    Latency_Dist_t *d = l->dists[(int)type];

    int bucket = 0;
    val >>= 1;
    while (val) {
        val >>= 1;
        ++bucket;
    }
    assert(bucket < LATENCY_NUM_BUCKETS);
    d->buckets[bucket] += count;

    return d;
}

static void
LatencyAdd(dmtr_timer_t *l, char type, uint64_t val)
{
    Latency_Dist_t *d = LatencyAddHist(l, type, val, 1);
    LatencyAddStat(l, type, val);

    if (val < d->min)
        d->min = val;
    if (val > d->max)
        d->max = val;
    d->total += val;
    ++d->count;
}

void
Latency_Sum(dmtr_timer_t *dest, dmtr_timer_t *summand)
{
    for (int i = 0; i < summand->distPoolNext; ++i) {
        Latency_Dist_t *d = &summand->distPool[i];
        for (int b = 0; b < LATENCY_NUM_BUCKETS; ++b) {
            if (d->buckets[b] == 0)
                continue;
            LatencyAddHist(dest, d->type, 1ll<<b, d->buckets[b]);
        }
    }

    for (int i = 0; i < LATENCY_MAX_DIST; ++i) {
        Latency_Dist_t *dd = dest->dists[i];
        Latency_Dist_t *ds = summand->dists[i];
        if (!ds)
            continue;

        if (ds->min < dd->min)
            dd->min = ds->min;
        if (ds->max > dd->max)
            dd->max = ds->max;
        dd->total += ds->total;
        dd->count += ds->count;
    }
}

static char *
LatencyFmtNS(uint64_t ns, char *buf)
{
    static const char *units[] = {"ns", "us", "ms", "s"};
    unsigned int unit = 0;
    while (ns >= 10000 && unit < (sizeof units / sizeof units[0]) - 1) {
        ns /= 1000;
        ++unit;
    }
    sprintf(buf, "%lu %s", ns, units[unit]);
    return buf;
}

int
Latency_Dump(FILE *f, dmtr_timer_t *l)
{
    DMTR_NOTNULL(EINVAL, f);
    DMTR_NOTNULL(EINVAL, l);

    if (l->distPoolNext == 0) {
        // No distributions yet
        return 0;
    }

    char buf[5][64];

    // Keep track of the index of the first used distribution, and
    // for each other used distribution, the index of the next
    // used distribution.  This way we only have to make one scan
    // over all the distributions and the rest of our scans
    // (especially when printing the histograms) are fast.
    int firstType = -1;
    int nextTypes[LATENCY_MAX_DIST];
    int *ppnext = &firstType;

    for (int type = 0; type < LATENCY_MAX_DIST; ++type) {
        Latency_Dist_t *d = l->dists[type];
        if (!d)
            continue;
        *ppnext = type;
        ppnext = &nextTypes[type];

        // Find the median
        uint64_t accum = 0;
        int medianBucket;
        for (medianBucket = 0; medianBucket < LATENCY_NUM_BUCKETS;
             ++medianBucket) {
            accum += d->buckets[medianBucket];
            if (accum >= d->count / 2)
                break;
        }

        char extra[3] = {'/', (char)type, 0};
        if (type == '=')
            extra[0] = '\0';
        fprintf(f, "LATENCY %s%s: %s %s/%s %s (%lu samples, %s total)\n",
                l->name.c_str(), extra, LatencyFmtNS(d->min, buf[0]),
                LatencyFmtNS(d->total / d->count, buf[1]),
                LatencyFmtNS((uint64_t)1 << medianBucket, buf[2]),
                LatencyFmtNS(d->max, buf[3]), d->count,
                LatencyFmtNS(d->total, buf[4]));
    }
    *ppnext = -1;
    l->latencies.shrink_to_fit();
    sort(l->latencies.begin(), l->latencies.end());
    fprintf(f, "TAIL LATENCY 99=%s 99.9=%s 99.99=%s\n",
	    LatencyFmtNS(l->latencies.at((int)((float)l->latencies.size() * 0.99)), buf[0]),
	    LatencyFmtNS(l->latencies.at((int)((float)l->latencies.size() * 0.999)), buf[1]),
	    LatencyFmtNS(l->latencies.at((int)((float)l->latencies.size() * 0.9999)), buf[2]));

    // Find the count of the largest bucket so we can scale the
    // histogram
    uint64_t largestCount = LATENCY_HISTOGRAM_WIDTH;
    for (int i = 0; i < LATENCY_NUM_BUCKETS; ++i) {
        uint64_t total = 0;
        for (int dist = 0; dist < l->distPoolNext; ++dist) {
            Latency_Dist_t *d = &l->distPool[dist];
            total += d->buckets[i];
        }
        if (total > largestCount)
            largestCount = total;
    }

    // Display the histogram
    int lastPrinted = LATENCY_NUM_BUCKETS;
    for (int i = 0; i < LATENCY_NUM_BUCKETS; ++i) {
        char bar[LATENCY_HISTOGRAM_WIDTH + 1];
        int pos = 0;
        uint64_t total = 0;
        for (int type = firstType; type != -1; type = nextTypes[type]) {
            Latency_Dist_t *d = l->dists[type];
            if (!d)
                continue;
            total += d->buckets[i];
            int goal = ((total * LATENCY_HISTOGRAM_WIDTH)
                        / largestCount);
            for (; pos < goal; ++pos)
                bar[pos] = type;
        }
        if (total > 0) {
            bar[pos] = '\0';
            if (lastPrinted < i - 3) {
                fprintf(f, "%10s |\n", "...");
            } else {
                for (++lastPrinted; lastPrinted < i;
                     ++lastPrinted)
                    fprintf(f, "%10s | %10ld |\n",
                            LatencyFmtNS((uint64_t)1 << lastPrinted,
                                         buf[0]), 0L);
            }
            fprintf(f, "%10s | %10ld | %s\n",
                    LatencyFmtNS((uint64_t)1 << i, buf[0]),
                    total,
                    bar);
            lastPrinted = i;
        }
    }

    return 0;
}

typedef dmtr_timer_t dmtr_timer_t;

int dmtr_new_timer(dmtr_timer_t **timer_out, const char *name) {
    DMTR_NOTNULL(EINVAL, timer_out);
    *timer_out = NULL;
    DMTR_NOTNULL(EINVAL, name);

    auto timer = new dmtr_timer_t();
    timer->name = name;
    timer->latencies.reserve(MAX_ITERATIONS);

    for (int i = 0; i < LATENCY_DIST_POOL_SIZE; ++i) {
        Latency_Dist_t *d = &timer->distPool[i];
        d->min = ~0ll;
    }

    *timer_out = timer;
    return 0;
}

int dmtr_start_timer(dmtr_timer_t *timer) {
    DMTR_NOTNULL(EINVAL, timer);

    timer->timer.start();
    return 0;
}

int dmtr_stop_timer(dmtr_timer_t *timer) {
    DMTR_NOTNULL(EINVAL, timer);

    auto elapsed = timer->timer.get_elapsed_ns();
    LatencyAdd(timer, '=', elapsed);
    return 0;
}

int dmtr_dump_timer(FILE *f, dmtr_timer_t *timer) {
    DMTR_OK(Latency_Dump(f, timer));
    return 0;
}

int dmtr_delete_timer(dmtr_timer_t **timer) {
    DMTR_NOTNULL(EINVAL, timer);

    delete *timer;
    *timer = NULL;
    return 0;
}
