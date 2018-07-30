/*********************************************************************************
*     File Name           :     measure.h
*     Created By          :     jingliu
**********************************************************************************/

#ifndef _LIBOS_MEASURE_H_
#define _LIBOS_MEASURE_H_


#include <cpuid.h>
#include <stdint.h>

/*** Low level interface ***/

/* there may be some unnecessary clobbering here*/
#define _setClockStart(HIs,LOs) {                                           \
asm volatile ("CPUID \n\t"                                                  \
              "RDTSC \n\t"                                                  \
              "mov %%edx, %0 \n\t"                                          \
              "mov %%eax, %1 \n\t":                                         \
              "=r" (HIs), "=r" (LOs)::                                      \
              "%rax", "%rbx", "%rcx", "%rdx");                              \
}

#define _setClockEnd(HIe,LOe) {                                             \
asm volatile ("RDTSCP \n\t"                                                 \
              "mov %%edx, %0 \n\t"                                          \
              "mov %%eax, %1 \n \t"                                         \
              "CPUID \n \t": "=r" (HIe), "=r" (LOe)::                       \
              "%rax", "%rbx", "%rcx", "%rdx");                              \
} 
#define _setClockBit(HIs,LOs,s,HIe,LOe,e) {                                 \
  s=LOs | ((uint64_t)HIs << 32);                                            \
  e=LOe | ((uint64_t)HIe << 32);                                            \
}


/*** High level interface ***/

typedef struct {
  volatile uint32_t hiStart;
  volatile uint32_t loStart;
  volatile uint32_t hiEnd;
  volatile uint32_t loEnd;
  volatile uint64_t tStart;
  volatile uint64_t tEnd;

  /*tend-tstart*/
  uint64_t tDur;
} timer_st;

#define startTimer(ts)                                                      \
{                                                                           \
  _setClockStart(ts.hiStart,ts.loStart);                                    \
} 


#define endTimer(ts)                                                        \
{                                                                           \
  _setClockEnd(ts.hiEnd,ts.loEnd);                                          \
  _setClockBit(ts.hiStart,ts.loStart,ts.tStart,                             \
      ts.hiEnd,ts.loEnd,ts.tEnd);                                           \
  ts.tDur=ts.tEnd-ts.tStart;                                                \
}                                                                             

#define lapTimer(ts)                                                        \
{                                                                           \
  ts.hiStart=ts.hiEnd;                                                      \
  ts.loStart=ts.loEnd;                                                      \
}


#endif  // _LIBOS_MEASURE_H_


