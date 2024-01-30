/*
 * Copyright (c) Microsoft Corporation.
 * Licensed under the MIT license.
 */

/*====================================================================================================================*
 * Imports                                                                                                            *
 *====================================================================================================================*/

#include "stopwatch.h"
#include <assert.h>
#include <demi/libos.h>
#include <demi/sga.h>
#include <demi/wait.h>
#include <stdbool.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

/*====================================================================================================================*
 * System Calls in demi/wait.h                                                                                        *
 *====================================================================================================================*/

/**
 * @brief Microbenchmark for demi_wait_any().
 */
static void microbench_wait_any(const unsigned NUM_ITERS, const unsigned NUM_QTS)
{
    demi_qtoken_t *qts = NULL;

    assert(NUM_ITERS > 0);
    assert(NUM_QTS > 0);

    // Allocate an array of queue tokens.
    assert((qts = malloc(sizeof(demi_qtoken_t) * NUM_QTS)) != NULL);

    stopwatch_reset();

    memset(qts, 1, sizeof(demi_qtoken_t) * NUM_QTS);

    for (unsigned i = 0; i < NUM_ITERS; i++)
    {
        demi_qresult_t qr = {};
        int ready_offset = -1;
        struct timespec timeout = {.tv_sec = 0, .tv_nsec = 0};

        stopwatch_start();
        assert(demi_wait_any(&qr, &ready_offset, qts, NUM_QTS, &timeout) != 0);
        stopwatch_stop();
    }

    printf("demi_wait_any() takes about %lld nanoseconds.\n", stopwatch_read());

    // Release resources.
    free(qts);
}

/*===================================================================================================================*
 * main()                                                                                                            *
 *===================================================================================================================*/

/**
 * @brief Drives the application.
 *
 * @param argc Argument count (unused).
 * @param argv Argument vector (unused).
 *
 * @return On successful completion EXIT_SUCCESS is returned.
 */
int main(int argc, char *const argv[])
{
    ((void)argc);
    ((void)argv);

    // This shall never fail.
    assert(demi_init(argc, argv) == 0);

    microbench_wait_any(100000, 1048576);

    return (EXIT_SUCCESS);
}
