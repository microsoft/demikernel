/*
 * measure.h
 *
 *  Created on: Aug 7, 2018
 *      Author: amanda
 */

#ifndef INCLUDE_MEASURE_H_
#define INCLUDE_MEASURE_H_

#include <stdint.h>
#include <time.h>

#define _CPUFREQ 3500LU /* MHz */

#define NS2CYCLE(__ns) (((__ns) * _CPUFREQ) / 1000)
#define CYCLE2NS(__cycles) (((__cycles) * 1000) / _CPUFREQ)

struct timer_info {
    double pop_duration;
    double dev_read_duration;
    double push_duration;
    double dev_write_duration;

    double pop_start;
    double push_end;
};

extern struct timer_info ti;

static inline double rdtsc(void)
{
//    uint32_t eax, edx;
//    __asm volatile ("rdtsc" : "=a" (eax), "=d" (edx) :: "memory");
//    return ((uint64_t)edx << 32) | eax;
    struct timespec tv;
    clock_gettime(CLOCK_REALTIME, &tv);
    return (tv.tv_sec*(uint64_t)1000000000+tv.tv_nsec) / 1000.0;
}

static inline void print_timer_info()
{
    double push_overhead = ti.push_duration - ti.dev_write_duration;
    double pop_overhead = ti.pop_duration - ti.dev_read_duration;
    double pop_to_push_duration = ti.push_end - ti.pop_start;

    printf("======================\n");
    printf("pop duration: %4.2f\n", ti.pop_duration);
    printf("read duration: %4.2f\n", ti.dev_read_duration);
    printf("push duration: %4.2f\n", ti.push_duration);
    printf("send duration: %4.2f\n", ti.dev_write_duration);
    printf("push overhead: %4.2f\n", push_overhead);
    printf("pop overhead: %4.2f\n", pop_overhead);
    printf("pop to push duration: %4.2f\n", pop_to_push_duration);
}

#endif /* INCLUDE_MEASURE_H_ */
