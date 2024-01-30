/*
 * Copyright (c) Microsoft Corporation.
 * Licensed under the MIT license.
 */

/*====================================================================================================================*
 * Imports                                                                                                            *
 *====================================================================================================================*/

// This must come first.
#define _POSIX_C_SOURCE 199309L

#include <assert.h>
#include <time.h>

/*====================================================================================================================*
 * Constants                                                                                                          *
 *====================================================================================================================*/

/**
 * @brief 10^9
 */
#define GIGA 1000000000

/**
 * @brief Number of iterations used to compute the average overhead of the stopwatch.
 */
#define NUM_ITER 100000

/*====================================================================================================================*
 * Private Variables                                                                                                  *
 *====================================================================================================================*/

/**
 * @brief Stopwatch.
 */
static struct
{
    struct timespec start;    /** Start time.      */
    struct timespec end;      /** End time.        */
    long nstops;              /** Number of stops. */
    long long total_time;     /** Total time.      */
    long long total_overhead; /** Total overhead.  */
} stopwatch = {.nstops = 0, .total_time = 0, .total_overhead = 0};

/*====================================================================================================================*
 * Public Functions                                                                                                   *
 *====================================================================================================================*/

/**
 * @brief Resets the stopwatch.
 */
void stopwatch_reset(void)
{
    stopwatch.nstops = 0;
    stopwatch.total_time = 0;
    stopwatch.total_overhead = 0;

    // Compute average overhead.
    for (int i = 0; i < NUM_ITER; i++)
    {
        assert(clock_gettime(CLOCK_MONOTONIC, &stopwatch.start) == 0);
        assert(clock_gettime(CLOCK_MONOTONIC, &stopwatch.end) == 0);

        stopwatch.total_overhead +=
            (stopwatch.end.tv_sec - stopwatch.start.tv_sec) * GIGA + (stopwatch.end.tv_nsec - stopwatch.start.tv_nsec);
    }

    stopwatch.total_overhead /= NUM_ITER;
}

/**
 * @brief Starts the stopwatch.
 */
void stopwatch_start(void)
{
    assert(clock_gettime(CLOCK_MONOTONIC, &stopwatch.start) == 0);
}

/**
 * @brief Stops the stopwatch.
 */
void stopwatch_stop(void)
{
    assert(clock_gettime(CLOCK_MONOTONIC, &stopwatch.end) == 0);

    stopwatch.nstops++;
    stopwatch.total_time += (stopwatch.end.tv_sec - stopwatch.start.tv_sec) * GIGA +
                            (stopwatch.end.tv_nsec - stopwatch.start.tv_nsec) - stopwatch.total_overhead;
}

/**
 * @brief Returns the elapsed times in nanoseconds.
 *
 * @return The elapsed times in nanoseconds.
 */
long long stopwatch_read(void)
{
    assert(stopwatch.nstops > 0);
    return (stopwatch.total_time / stopwatch.nstops);
}
