/*
 * Copyright (c) Microsoft Corporation.
 * Licensed under the MIT license.
 */

#ifndef STOPWATCH_H_
#define STOPWATCH_H_

/**
 * @brief Resets the stopwatch.
 */
extern void stopwatch_reset(void);

/**
 * @brief Starts the stopwatch.
 */
extern void stopwatch_start(void);

/**
 * @brief Stops the stopwatch.
 */
extern void stopwatch_stop(void);

/**
 * @brief Returns the elapsed times in nanoseconds.
 *
 * @return The elapsed times in nanoseconds.
 */
extern long long stopwatch_read(void);

#endif /* !STOPWATCH_H_ */
