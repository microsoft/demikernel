/*
 * Copyright (c) Microsoft Corporation.
 * Licensed under the MIT license.
 */

/*====================================================================================================================*
 * Imports                                                                                                            *
 *====================================================================================================================*/

#include <demi/types.h>
#include <stdio.h>
#include <stdlib.h>

/*====================================================================================================================*
 * Macro Functions                                                                                                    *
 *====================================================================================================================*/

/**
 * @brief Returns the maximum of two values.
 *
 * @param a First value.
 * @param b Second value.
 *
 * @returns The maximum of 'a' and 'b'.
 */
#define MAX(a, b) (((a) > (b)) ? (a) : (b))

/**
 * @brief Asserts if 'a' and 'b' agree on size.
 *
 * @param a Probing size.
 * @param b Control size.
 *
 * @returns Upon success, compilation proceeds as normal. Upon failure, a
 * compilation error is generated.
 */
#ifdef _WIN32
#define KASSERT_SIZE(a, b) static_assert((a) == (b), "Size mismatch.")
#else
#define KASSERT_SIZE(a, b) ((void)sizeof(char[(((a) == (b)) ? 1 : -1)]))
#endif

/*====================================================================================================================*
 * Constants                                                                                                          *
 *====================================================================================================================*/

// The following sizes are intentionally hardcoded.
#define SGASEG_BUF_SIZE 8
#define SGASEG_LEN_SIZE 4
#define DEMI_SGASEG_T_SIZE (SGASEG_BUF_SIZE + SGASEG_LEN_SIZE)
#define SGA_BUF_SIZE 8
#define SGA_NUMSEGS_SIZE 4
#define SGA_SEGS_SIZE (DEMI_SGASEG_T_SIZE * DEMI_SGARRAY_MAXSIZE)
#define SGA_ADDR_SIZE 16
#define DEMI_SGARRAY_T_SIZE (SGA_BUF_SIZE + SGA_NUMSEGS_SIZE + SGA_SEGS_SIZE + SGA_ADDR_SIZE)
#define QD_SIZE 4
#define SADDR_SIZE 16
#define DEMI_ACCEPT_RESULT_T_SIZE (QD_SIZE + SADDR_SIZE)
#define QR_OPCODE_SIZE 4
#define QR_QD_SIZE 4
#define QR_QT_SIZE 8
#define QR_RET_SIZE 8
#define QR_VALUE_SIZE (MAX(DEMI_ACCEPT_RESULT_T_SIZE, DEMI_SGARRAY_T_SIZE))
#define DEMI_QRESULT_T_SIZE (QR_OPCODE_SIZE + QR_QD_SIZE + QR_QT_SIZE + QR_RET_SIZE + QR_VALUE_SIZE)
#define DEMI_ARGS_ARGC_SIZE 4
#define DEMI_ARGS_ARGV_SIZE 8
#define DEMI_ARGS_CALLBACK_SIZE 8
#define DEMI_ARGS_SIZE (DEMI_ARGS_ARGC_SIZE + DEMI_ARGS_ARGV_SIZE + DEMI_ARGS_CALLBACK_SIZE)

/*====================================================================================================================*
 * Private Functions                                                                                                  *
 *====================================================================================================================*/

/**
 * @brief tests if @p demi_sgaseg_t has the expected size.
 *
 * @note This is a compile-time-test.
 */
static void test_size_sgaseg_t(void)
{
    KASSERT_SIZE(sizeof(demi_sgaseg_t), DEMI_SGASEG_T_SIZE);
    printf("sizeof(demi_sgaseg_t) = %zu\n", sizeof(demi_sgaseg_t));
}

/**
 * @brief Tests if @p demi_sga_t has the expected size.
 *
 * @note This is a compile-time-test.
 */
static void test_size_sga_t(void)
{
    KASSERT_SIZE(sizeof(demi_sgarray_t), DEMI_SGARRAY_T_SIZE);
    printf("sizeof(demi_sgarray_t) = %zu\n", sizeof(demi_sgarray_t));
}

/**
 * @brief Tests if @p demi_accept_result_t has the expected size.
 *
 * @note This is a compile-time-test.
 */
static void test_size_demi_accept_result_t(void)
{
    KASSERT_SIZE(sizeof(demi_accept_result_t), DEMI_ACCEPT_RESULT_T_SIZE);
    printf("sizeof(demi_accept_result_t) = %zu\n", sizeof(demi_accept_result_t));
}

/**
 * @brief Tests if demi_qresult_t has the expected size.
 *
 * @note This is a compile-time-test.
 */
static void test_size_demi_qresult_t(void)
{
    KASSERT_SIZE(sizeof(demi_qresult_t), DEMI_QRESULT_T_SIZE);
    printf("sizeof(demi_qresult_t) = %zu\n", sizeof(demi_qresult_t));
}

/**
 * @brief Tests if demi_args_t has the expected size.
 */
static void test_size_demi_args_t(void)
{
    KASSERT_SIZE(sizeof(struct demi_args), DEMI_ARGS_SIZE);
    printf("sizeof(demi_args_t) = %zu\n", sizeof(struct demi_args));
}

/*====================================================================================================================*
 * Public Functions                                                                                                   *
 *====================================================================================================================*/

/**
 * @brief Drives the application.
 *
 * This system-level conducts tests to ensure that the sizes of structures and
 * data types exposed by the C bindings of Demikernel are correct.
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

    test_size_sgaseg_t();
    test_size_sga_t();
    test_size_demi_accept_result_t();
    test_size_demi_qresult_t();
    test_size_demi_args_t();

    return (EXIT_SUCCESS);
}
