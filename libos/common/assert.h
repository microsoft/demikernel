// -*- mode: c++; c-file-style: "k&r"; c-basic-offset: 4 -*-
/***********************************************************************
 *
 * assert.h:
 *   assertion macros that integrate with the logging framework
 *
 * Copyright 2013-2015 Irene Zhang <iyzhang@cs.washington.edu>
 *                     Naveen Kr. Sharma <naveenks@cs.washington.edu>
 *                     Dan R. K. Ports  <drkp@cs.washington.edu>
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

#ifndef _LIB_ASSERT_H_
#define _LIB_ASSERT_H_

/*
 * Assertion macros.
 *
 * Currently these mostly just wrap the standard C assert but
 * eventually they should tie in better with the logging framework.
 */
#include <assert.h>
#include <stdlib.h>
#include <stdio.h>
#include "lib/message.h"

#define ASSERT(x) Assert(x)

// XXX These should output the expected and actual values in addition
// to failing.
#define ASSERT_EQ(x, y) Assert(x == y)
#define ASSERT_LT(x, y) Assert(x < y)
#define ASSERT_GT(x, y) Assert(x > y)
#define ASSERT_LE(x, y) Assert(x <= y)
#define ASSERT_GE(x, y) Assert(x >= y)

#define NOT_REACHABLE() do {                                            \
        fprintf(stderr, "NOT_REACHABLE point reached: %s, line %d\n",   \
                __FILE__, __LINE__);                                    \
        abort();                                                        \
    } while (0)

#define NOT_IMPLEMENTED() do {                                          \
        fprintf(stderr, "NOT_IMPLEMENTED point reached: %s, line %d\n", \
                __FILE__, __LINE__);                                    \
        abort();                                                        \
} while (0)


#endif  /* _LIB_ASSERT_H */
