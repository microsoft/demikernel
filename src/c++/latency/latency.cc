// -*- mode: c++; c-file-style: "k&r"; c-basic-offset: 4 -*-

// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#include <dmtr/latency.h>

#include <algorithm>
#include <cassert>
#include <dmtr/annot.h>
#include <dmtr/fail.h>
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

typedef boost::chrono::duration<uint64_t, boost::nano> duration_type;

typedef struct Latency_Dist_t
{
    uint64_t min, max, total, count;
    uint32_t buckets[LATENCY_NUM_BUCKETS];
    char type;
} Latency_Dist_t;

typedef struct dmtr_latency
{
    std::string name;

    Latency_Dist_t *dists[LATENCY_MAX_DIST];
    Latency_Dist_t distPool[LATENCY_DIST_POOL_SIZE];
    int distPoolNext = 0;

    std::vector<uint64_t> latencies;
    std::vector<uint64_t> record_times;
} dmtr_latency_t;

static inline void
LatencyAddStat(dmtr_latency_t *l, char type, uint64_t val, uint64_t record_time)
{
    //if (l->latencies.size() == 0)

    if (l->latencies.size() < MAX_ITERATIONS) {
        l->latencies.push_back(val);
    }
    if (record_time) {
        l->record_times.push_back(record_time);
    }
}

static inline Latency_Dist_t *
LatencyAddHist(dmtr_latency_t *l, char type, uint64_t val, uint32_t count)
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
LatencyAdd(dmtr_latency_t *l, char type, uint64_t val, uint64_t record_time)
{
    Latency_Dist_t *d = LatencyAddHist(l, type, val, 1);
    LatencyAddStat(l, type, val, record_time);

    if (val < d->min)
        d->min = val;
    if (val > d->max)
        d->max = val;
    d->total += val;
    ++d->count;
}

void
Latency_Sum(dmtr_latency_t *dest, dmtr_latency_t *summand)
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
LatencyToCsv(const char *filename, dmtr_latency_t *l)
{
    DMTR_NOTNULL(EINVAL, l);
    FILE *f = fopen(filename, "w");
    DMTR_NOTNULL(EINVAL, f);
    fprintf(f, "TIME\tVALUE\n");
    for (unsigned int i = 0; i < l->latencies.size(); ++i) {
        fprintf(f, "%ld\t%ld\n", l->record_times[i], l->latencies[i]);
    }
    DMTR_OK(fclose(f));
    return 0;
}

int
Latency_Dump(FILE *f, dmtr_latency_t *l)
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

int dmtr_new_latency(dmtr_latency_t **latency_out, const char *name) {
    DMTR_NOTNULL(EINVAL, latency_out);
    *latency_out = NULL;
    DMTR_NOTNULL(EINVAL, name);

    auto latency = new dmtr_latency_t();
    latency->name = name;
    latency->latencies.reserve(MAX_ITERATIONS);

    for (int i = 0; i < LATENCY_DIST_POOL_SIZE; ++i) {
        Latency_Dist_t *d = &latency->distPool[i];
        d->min = ~0ll;
    }

    *latency_out = latency;
    return 0;
}

int dmtr_record_timed_latency(dmtr_latency_t *latency, uint64_t record_time, uint64_t ns) {
    DMTR_NOTNULL(EINVAL, latency);
    DMTR_NONZERO(EINVAL, ns);

    LatencyAdd(latency, '=', ns, record_time);
    return 0;
}

int dmtr_record_latency(dmtr_latency_t *latency, uint64_t ns) {
    DMTR_NOTNULL(EINVAL, latency);
    DMTR_NONZERO(EINVAL, ns);

    LatencyAdd(latency, '=', ns, -1);
    return 0;
}

int dmtr_dump_latency_to_file(const char *filename, dmtr_latency_t *latency) {
    DMTR_OK(LatencyToCsv(filename, latency));
    return 0;
}

int dmtr_dump_latency(FILE *f, dmtr_latency_t *latency) {
    DMTR_OK(Latency_Dump(f, latency));

    return 0;
}

int dmtr_delete_latency(dmtr_latency_t **latency) {
    DMTR_NOTNULL(EINVAL, latency);

    delete *latency;
    *latency = NULL;
    return 0;
}

uint64_t dmtr_now_ns() {
    auto t = boost::chrono::steady_clock::now();
    return t.time_since_epoch().count();
}

long int since_epoch(boost::chrono::steady_clock::time_point &time) {
    return boost::chrono::time_point_cast<boost::chrono::nanoseconds>(time).time_since_epoch().count();
}

long int ns_diff(boost::chrono::steady_clock::time_point &start,
                               boost::chrono::steady_clock::time_point &end) {
    auto ns = boost::chrono::duration_cast<boost::chrono::nanoseconds>(end-start).count();
    if (ns < 0) {
        ns = -1;
    }
    return ns;
}

int dmtr_register_latencies(const char *label,
                            std::unordered_map<pthread_t, latency_ptr_type> &latencies) {
    char log_filename[MAX_LOG_FILENAME_LEN];
    const char *log_dir = dmtr_log_directory.c_str();
    pthread_t me = pthread_self();

    auto it = latencies.find(me);
    DMTR_TRUE(EINVAL, it == latencies.end());
    snprintf(log_filename, MAX_LOG_FILENAME_LEN, "%s/%ld-%s-latencies", log_dir, me, label);
    dmtr_latency_t *l;
    DMTR_OK(dmtr_new_latency(&l, label));
    latency_ptr_type latency =
        latency_ptr_type(l, [log_filename](dmtr_latency_t *lat) {
            dmtr_dump_latency_to_file(reinterpret_cast<const char *>(log_filename), lat);
            dmtr_dump_latency(stderr, lat);
            dmtr_delete_latency(&lat);
        });
    latencies.insert(
        std::pair<pthread_t, latency_ptr_type>(me, std::move(latency))
    );

    return 0;
}
