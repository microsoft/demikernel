/* 	rdtsc.h
	
	Iterface to the x86 cycle counter
	Usage:
	1) Include the header file
	#include "rdtsc.h"
	2) Set the machine frequency (for instace on a 3.2 GHz machine)
	#define PROCESSOR_FREQ	3200E6
	3) Declare counter variables in your code
	tsc_counter t0, t1, t_min;
	4) You can set a counter to the mamximum number of cycles
	COUNTER_VAL(t_min) = 0xFFFFFFFFFFFF;
	5) Measure something by two invocations to RDTSC() with two different variables 
	RDTSC(t0); 
	...do work here 
	RDTSC(t1);
	6) Compute the difference of counter vals and set values
	COUNTER_VAL(t_min) = MIN(COUNTER_VAL(t_min), COUNTER_DIFF(t1, t0));
	7) Print the the elapsed time for a short interval (less than 2 billion cycles) 
	printf("%d%, (int)COUNTER_LO(t_min));
*/

#define COUNTER_LO(a) ((a).int32.lo)
#define COUNTER_HI(a) ((a).int32.hi)
#define COUNTER_VAL(a) ((a).int64)

#define COUNTER_DIFF(a,b) \
	(COUNTER_VAL(a)-COUNTER_VAL(b))

/* ==================== GNU C and possibly other UNIX compilers ===================== */
#ifndef WIN32

#ifdef __GNUC__
#define VOLATILE __volatile__
#define ASM __asm__
#else
/* we can at least hope the following works, it probably won't */
#define ASM asm
#define VOLATILE 
#endif

#define INT64 unsigned long long
#define INT32 unsigned int

typedef union
{       INT64 int64;
        struct {INT32 lo, hi;} int32;
} tsc_counter;

#define RDTSC(cpu_c) \
 ASM VOLATILE ("rdtsc" : "=a" ((cpu_c).int32.lo), "=d"((cpu_c).int32.hi))
#define CPUID(x) \
 ASM VOLATILE ("cpuid" : "=a" (x) : "0" (x) : "bx", "cx", "dx" )

int rdtsc_works() {
    tsc_counter dummy;
    RDTSC(dummy);
    /* I don't know how to do this right */
    return 1;
}

/* ======================== WIN32 ======================= */
#else

#define INT64 signed __int64
#define INT32 unsigned __int32

typedef union
{       INT64 int64;
        struct {INT32 lo, hi;} int32;
} tsc_counter;

#define RDTSC(cpu_c)   \
{       __asm rdtsc    \
        __asm mov (cpu_c).int32.lo,eax  \
        __asm mov (cpu_c).int32.hi,edx  \
}

#define CPUID(x) \
{ \
    __asm mov eax, x \
    __asm cpuid \
    __asm mov x, eax \
}

int rdtsc_works()
{
    tsc_counter dummy;
    __try {
	RDTSC(dummy);
    } __except ( 1) {
	return 0;
    }
    return 1;
}
#endif

/*
#define RDTSC(cpu_c) \
{	asm("rdtsc"); 	\
	asm("mov %%eax, %0" : "=m" ((cpu_c).int32.lo) ); \
	asm("mov %%edx, %0" : "=m" ((cpu_c).int32.hi) ); \
}
*/

/*	Read Time Stamp Counter
	Read PMC 
#define RDPMC0(cpu_c) \
{		     	\
        __asm xor ecx,ecx	\
	__asm rdpmc	\
	__asm mov (cpu_c).int32.lo,eax	\
	__asm mov (cpu_c).int32.hi,edx	\
}
*/
