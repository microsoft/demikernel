/*
 * Copyright (c) Microsoft Corporation.
 * Licensed under the MIT license.
 */

#include <assert.h>
#include <demi/libos.h>
#include <demi/sga.h>
#include <demi/wait.h>
#include <stdbool.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

/*===================================================================================================================*
 * System Calls in demi/libos.h                                                                                      *
 *===================================================================================================================*/

/**
 * @brief Issues an invalid call to demi_socket().
 */
static bool inval_socket(void)
{
    int *qd = NULL;
    int domain = -1;
    int type = -1;
    int protocol = -1;

    return (demi_socket(qd, domain, type, protocol) != 0);
}

/**
 * @brief Issues an invalid call to demi_listen().
 */
static bool inval_listen(void)
{
    int qd = -1;
    int backlog = -1;

    return (demi_listen(qd, backlog) != 0);
}

/**
 * @brief Issues an invalid call to demi_bind().
 */
static bool inval_bind(void)
{
    int qd = -1;
    struct sockaddr *saddr = NULL;
    socklen_t size = 0;

    return (demi_bind(qd, saddr, size) != 0);
}

/**
 * @brief Issues an invalid call to demi_accept().
 */
static bool inval_accept(void)
{
    demi_qtoken_t *qt = NULL;
    int sockqd = -1;

    return (demi_accept(qt, sockqd) != 0);
}

/**
 * @brief Issues an invalid call to demi_connect().
 */
static bool inval_connect(void)
{
    demi_qtoken_t *qt = NULL;
    int qd = -1;
    struct sockaddr *saddr = NULL;
    socklen_t size = -1;

    return (demi_connect(qt, qd, saddr, size) != 0);
}

/**
 * @brief Issues an invalid call to demi_close().
 */
static bool inval_close(void)
{
    int qd = -1;

    return (demi_close(qd) != 0);
}

/**
 * @brief Issues an invalid call to demi_push().
 */
static bool inval_push(void)
{
    demi_qtoken_t *qt = NULL;
    int qd = -1;
    demi_sgarray_t *sga = NULL;

    return (demi_push(qt, qd, sga) != 0);
}

/**
 * @brief Issues an invalid call to demi_pushto().
 */
static bool inval_pushto(void)
{
    demi_qtoken_t *qt = NULL;
    int qd = -1;
    demi_sgarray_t *sga = NULL;
    struct sockaddr *saddr = NULL;
    socklen_t size = -1;

    return (demi_pushto(qt, qd, sga, saddr, size) != 0);
}

/**
 * @brief Issues an invalid call to demi_pop().
 */
static bool inval_pop(void)
{
    demi_qtoken_t *qt = NULL;
    int qd = -1;

    return (demi_pop(qt, qd) != 0);
}

/*===================================================================================================================*
 * System Calls in demi/sga.h                                                                                        *
 *===================================================================================================================*/

/**
 * @brief Issues an invalid call to demi_sgaalloc().
 */
static bool inval_sgaalloc(void)
{
    size_t len = 0;

    demi_sgarray_t sga = demi_sgaalloc(len);
    return (sga.sga_buf == NULL);
}

/**
 * @brief Issues an invalid call to demi_sgafree().
 */
static bool inval_sgafree(void)
{
    demi_sgarray_t *sga = NULL;

    return (demi_sgafree(sga) != 0);
}

/*===================================================================================================================*
 * System Calls in demi/wait.h                                                                                       *
 *===================================================================================================================*/

/**
 * @brief Issues an invalid system call to demi_timedwait().
 */
static bool inval_timedwait(void)
{
    struct timespec *abstime = NULL;
    demi_qresult_t *qr = NULL;
    demi_qtoken_t qt = -1;

    return (demi_timedwait(qr, qt, abstime) != 0);
}

/**
 * @brief Issues an invalid system call to demi_wait().
 */
static bool inval_wait(void)
{
    demi_qresult_t *qr = NULL;
    demi_qtoken_t qt = -1;
    struct timespec *timeout = NULL;

    return (demi_wait(qr, qt, timeout) != 0);
}

/**
 * @brief Issues an invalid system call to demi_wait_any().
 */
static bool inval_wait_any(void)
{
    demi_qresult_t *qr = NULL;
    int *ready_offset = NULL;
    demi_qtoken_t *qts = NULL;
    int num_qts = -1;
    struct timespec *timeout = NULL;

    return (demi_wait_any(qr, ready_offset, qts, num_qts, timeout) != 0);
}

/*===================================================================================================================*
 * main()                                                                                                            *
 *===================================================================================================================*/

/**
 * @brief Tests Descriptor
 */
struct test
{
    bool (*fn)(void); /**<  Test Function */
    const char *name; /**< Test Name      */
};

/**
 * @brief Tests for system calls in demi/libos.h
 */
static struct test tests_libos[] = {{inval_socket, "invalid demi_socket()"},   {inval_accept, "invalid demi_accept()"},
                                    {inval_bind, "invalid demi_bind()"},       {inval_close, "invalid_demi_close()"},
                                    {inval_connect, "invalid demi_connect()"}, {inval_listen, "invalid demi_listen()"},
                                    {inval_pop, "invalid demi_pop()"},         {inval_push, "invalid demi_push()"},
                                    {inval_pushto, "invalid demi_pushto()"}};

/**
 * @brief Tests for system calls in demi/sga.h
 */
static struct test tests_sga[] = {{inval_sgaalloc, "invalid demi_sgaalloc()"},
                                  {inval_sgafree, "invalid demi_sgafree()"}};

/**
 * @brief Tests for system calls in demi/wait.h
 */
static struct test tests_wait[] = {{inval_timedwait, "invalid demi_timedwait()"},
                                   {inval_wait, "invalid demi_wait()"},
                                   {inval_wait_any, "invalid demi_wait_any()"}};

/**
 * @brief Drives the application.
 *
 * This system-level test issues invalid calls to all system calls in Demikernel.
 * Overall, this test ensures the following points:
 * - All system calls of Demikernel are callable from a C application.
 * - All invalid system calls fail.
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

    /* This shall never fail. */
    assert(demi_init(argc, argv) == 0);

    /* System calls in demi/libos.h */
    for (size_t i = 0; i < sizeof(tests_libos) / sizeof(struct test); i++)
    {
        if (tests_libos[i].fn() == true)
            fprintf(stderr, "test result: passed %s\n", tests_libos[i].name);
        else
        {
            fprintf(stderr, "test result: FAILED %s\n", tests_libos[i].name);
            return (EXIT_FAILURE);
        }
    }

    /* System calls in demi/sga.h */
    for (size_t i = 0; i < sizeof(tests_sga) / sizeof(struct test); i++)
    {
        if (tests_sga[i].fn() == true)
            fprintf(stderr, "test result: passed %s\n", tests_sga[i].name);
        else
        {
            fprintf(stderr, "test result: FAILED %s\n", tests_sga[i].name);
            return (EXIT_FAILURE);
        }
    }

    /* System calls in demi/wait.h */
    for (size_t i = 0; i < sizeof(tests_wait) / sizeof(struct test); i++)
    {
        if (tests_wait[i].fn() == true)
            fprintf(stderr, "test result: passed %s\n", tests_wait[i].name);
        else
        {
            fprintf(stderr, "test result: FAILED %s\n", tests_wait[i].name);
            return (EXIT_FAILURE);
        }
    }

    return (EXIT_SUCCESS);
}
