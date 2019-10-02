// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.
#ifndef DMTR_TIME_H_IS_INCLUDED
#define DMTR_TIME_H_IS_INCLUDED

#include <boost/chrono.hpp>
using hr_clock = boost::chrono::steady_clock;
typedef boost::chrono::steady_clock::time_point tp;

uint64_t since_epoch(tp &time);
uint64_t ns_diff(tp &start, tp &end);
tp take_time();

#endif /* DMTR_TIME_H_IS_INCLUDED */
