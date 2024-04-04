// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#include <stdarg.h>
#include <stdbool.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

//======================================================================================================================
// Constants
//======================================================================================================================

/**
 * @brief Maximum length for a log message.
 */
#define LOG_MAX_LEN 256

/**
 * @brief Number of log levels.
 */
#define LOG_LEVELS_NUM 7

//======================================================================================================================
// Global Variables
//======================================================================================================================

/**
 * @brief Current log level.
 */
static int curr_level = LOG_LEVELS_NUM;

/**
 * @brief Name for log levels.
 */
static const char *level_names[LOG_LEVELS_NUM] = {
    "NONE"
    "TRACE",
    "DEBUG",
    "INFO",
    "WARN",
    "ERROR",
    "PANIC",
};

//======================================================================================================================
// Global Functions
//======================================================================================================================

/**
 * @brief Outputs a log message.
 *
 * @param level Log level.
 * @param file  Name of the file that emitted the log.
 * @param line  Line number where the log was emitted.
 * @param func  Name of the function that emitted the log.
 * @param fmt   Formatted log message.
 */
void __log(int level, const char *file, int line, const char *func, const char *fmt, ...)
{
    static bool initialized = false;

    // Initializes the log device.
    if (!initialized)
    {
        const char *env = getenv("C_LOG");
        if (env != NULL)
        {
            if (!strcmp(env, "trace"))
                curr_level = 1;
            else if (!strcmp(env, "debug"))
                curr_level = 2;
            else if (!strcmp(env, "info"))
                curr_level = 3;
            else if (!strcmp(env, "warn"))
                curr_level = 4;
            else if (!strcmp(env, "error"))
                curr_level = 5;
        }

        initialized = true;
    }

    // Print log message.
    if ((level >= curr_level) && (level < LOG_LEVELS_NUM))
    {
        char msg[LOG_MAX_LEN];
        va_list args;
        va_start(args, fmt);
        vsnprintf(msg, LOG_MAX_LEN, fmt, args);
        va_end(args);
        fprintf(stderr, "%-5s [%s:%d] %s(): %s\n", level_names[level], file, line, func, msg);
    }
}
