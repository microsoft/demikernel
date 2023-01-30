// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#ifndef _LOG_H_
#define _LOG_H_

/**
 * @brief Log levels.
 */
/**@{*/
#define LOG_TRACE 1 /**< Trace.   */
#define LOG_DEBUG 2 /**< Debug.   */
#define LOG_INFO 3  /**< Info.    */
#define LOG_WARN 4  /**< Warning. */
#define LOG_ERROR 5 /**< Error.   */
#define LOG_PANIC 5 /**< Panic.   */
/**@}*/

/**
 * @brief Emits a log message
 *
 * @param level Log level.
 * @param fmt   Formatted log message.
 */
#define LOG(level, fmt, ...) __log(level, __FILE__, __LINE__, __func__, fmt, ##__VA_ARGS__)

/**
 * @brief Emits a trace trace message
 *
 * @param fmt Formatted log message.
 */
#define TRACE(fmt, ...) LOG(LOG_TRACE, fmt, ##__VA_ARGS__)

/**
 * @brief Emits a debug message
 *
 * @param fmt Formatted debug message.
 */
#define DEBUG(fmt, ...) LOG(LOG_DEBUG, fmt, ##__VA_ARGS__)

/**
 * @brief Emits an informational  message
 *
 * @param fmt Formatted informational message.
 */
#define INFO(fmt, ...) LOG(LOG_INFO, fmt, ##__VA_ARGS__)

/**
 * @brief Emits a warning message
 *
 * @param fmt Formatted warning message.
 */
#define WARN(fmt, ...) LOG(LOG_WARN, fmt, ##__VA_ARGS__)

/**
 * @brief Emits an error log message
 *
 * @param fmt Formatted error message.
 */
#define ERROR(fmt, ...) LOG(LOG_ERROR, fmt, ##__VA_ARGS__)

extern void __log(int level, const char *file, int line, const char *func, const char *fmt, ...);

#endif /* _LOG_H_ */
