// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#ifndef _ERROR_H_
#define _ERROR_H_

#include "log.h"
#include <stdlib.h>

/**
 * @brief Panics because unreachable code was entered.
 *
 * @param msg Panic message.
 */
#define UNREACHABLE(msg)                                                                                               \
    {                                                                                                                  \
        LOG(LOG_PANIC, "unreachable: %s", msg);                                                                        \
        abort();                                                                                                       \
    }

/**
 * @brief Panics because some unimplemented functionality was requested.
 *
 * @param msg Panic message.
 */
#define UNIMPLEMETED(msg)                                                                                              \
    {                                                                                                                  \
        LOG(LOG_PANIC, "unimplemented : %s", msg);                                                                     \
        abort();                                                                                                       \
    }

/**
 * @brief Outputs a panic message and aborts execution.
 *
 * @param fmt Formatted panic message.
 */
#define PANIC(fmt, ...)                                                                                                \
    {                                                                                                                  \
        abort();                                                                                                       \
    }

#endif /* _ERROR_H_ */
