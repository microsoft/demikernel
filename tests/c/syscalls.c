/*
 * Copyright (c) Microsoft Corporation.
 * Licensed under the MIT license.
 */

#include <assert.h>
#include <dmtr/libos.h>
#include <dmtr/sga.h>
#include <dmtr/wait.h>
#include <stdbool.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

/*===================================================================================================================*
 * System Calls in dmtr/libos.h                                                                                      *
 *===================================================================================================================*/

/**
 * @brief Issues an invalid call to dmtr_socket().
 */
static bool inval_socket(void)
{
    int *qd = NULL;
    int domain = -1;
    int type = -1;
    int protocol = -1;

    return (dmtr_socket(qd, domain, type, protocol) != 0);
}

/**
 * @brief Issues an invalid call to dmtr_getsockname().
 */
static bool inval_getsockname(void)
{
    int qd = -1;
    struct sockaddr *saddr = NULL;
    socklen_t *size = NULL;

    return (dmtr_getsockname(qd, saddr, size) != 0);
}

/**
 * @brief Issues an invalid call to dmtr_listen().
 */
static bool inval_listen(void)
{
    int qd = -1;
    int backlog = -1;

    return (dmtr_listen(qd, backlog) != 0);
}

/**
 * @brief Issues an invalid call to dmtr_bind().
 */
static bool inval_bind(void)
{
    int qd = -1;
    struct sockaddr *saddr = NULL;
    socklen_t size = 0;

    return (dmtr_bind(qd, saddr, size) != 0);
}

/**
 * @brief Issues an invalid call to dmtr_accept().
 */
static bool inval_accept(void)
{
    dmtr_qtoken_t *qt = NULL;
    int sockqd = -1;

    return (dmtr_accept(qt, sockqd) != 0);
}

/**
 * @brief Issues an invalid call to dmtr_connect().
 */
static bool inval_connect(void)
{
    dmtr_qtoken_t *qt = NULL;
    int qd = -1;
    struct sockaddr *saddr = NULL;
    socklen_t size = -1;

    return (dmtr_connect(qt, qd, saddr, size) != 0);
}

/**
 * @brief Issues an invalid call to dmtr_close().
 */
static bool inval_close(void)
{
    int qd = -1;

    return (dmtr_close(qd) != 0);
}

/**
 * @brief Issues an invalid call to dmtr_push().
 */
static bool inval_push(void)
{
    dmtr_qtoken_t *qt = NULL;
    int qd = -1;
    demi_sgarray_t *sga = NULL;

    return (dmtr_push(qt, qd, sga) != 0);
}

/**
 * @brief Issues an invalid call to dmtr_pushto().
 */
static bool inval_pushto(void)
{
    dmtr_qtoken_t *qt = NULL;
    int qd = -1;
    demi_sgarray_t *sga = NULL;
    struct sockaddr *saddr = NULL;
    socklen_t size = -1;

    return (dmtr_pushto(qt, qd, sga, saddr, size) != 0);
}

/**
 * @brief Issues an invalid call to dmtr_pop().
 */
static bool inval_pop(void)
{
    dmtr_qtoken_t *qt = NULL;
    int qd = -1;

    return (dmtr_pop(qt, qd) != 0);
}

/*===================================================================================================================*
 * System Calls in dmtr/sga.h                                                                                        *
 *===================================================================================================================*/

/**
 * @brief Issues an invalid call to dmtr_sgaalloc().
 */
static bool inval_sgaalloc(void)
{
    size_t len = 0;

    demi_sgarray_t sga = dmtr_sgaalloc(len);
    return (sga.sga_buf == NULL);
}

/**
 * @brief Issues an invalid call to dmtr_sgafree().
 */
static bool inval_sgafree(void)
{
    demi_sgarray_t *sga = NULL;

    return (dmtr_sgafree(sga) != 0);
}

/*===================================================================================================================*
 * System Calls in dmtr/wait.h                                                                                       *
 *===================================================================================================================*/

/**
 * @brief Issues an invalid system call to dmtr_wait().
 *
 * TODO: Enable this test once we fix https://github.com/demikernel/scheduler/issues/6.
 */
static bool inval_wait(void)
{
#if 0
    demi_qresult_t *qr = NULL;
    dmtr_qtoken_t qt = -1;

    return (dmtr_wait(qr, qt) != 0);
#else
    return (true);
#endif
}

/**
 * @brief Issues an invalid system call to dmtr_wait_any().
 */
static bool inval_wait_any(void)
{
    demi_qresult_t *qr = NULL;
    int *ready_offset = NULL;
    dmtr_qtoken_t *qts = NULL;
    int num_qts = -1;

    return (dmtr_wait_any(qr, ready_offset, qts, num_qts) != 0);
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
 * @brief Tests for system calls in dmtr/libos.h
 */
static struct test tests_libos[] = {
    {inval_socket, "invalid dmtr_socket()"},   {inval_accept, "invalid dmtr_accept()"},
    {inval_bind, "invalid dmtr_bind()"},       {inval_close, "invalid_dmtr_close()"},
    {inval_connect, "invalid dmtr_connect()"}, {inval_getsockname, "invalid dmtr_getsockname()"},
    {inval_listen, "invalid dmtr_listen()"},   {inval_pop, "invalid dmtr_pop()"},
    {inval_push, "invalid dmtr_push()"},       {inval_pushto, "invalid dmtr_pushto()"}};

/**
 * @brief Tests for system calls in dmtr/sga.h
 */
static struct test tests_sga[] = {{inval_sgaalloc, "invalid dmtr_sgaalloc()"},
                                  {inval_sgafree, "invalid dmtr_sgafree()"}};

/**
 * @brief Tests for system calls in dmtr/wait.h
 */
static struct test tests_wait[] = {{inval_wait, "invalid dmtr_wait()"}, {inval_wait_any, "invalid dmtr_wait_any()"}};

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
int main(int argc, char **argv)
{
    ((void)argc);
    ((void)argv);

    /* This shall never fail. */
    assert(dmtr_init(argc, argv) == 0);

    /* System calls in dmtr/libos.h */
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

    /* System calls in dmtr/sga.h */
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

    /* System calls in dmtr/wait.h */
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