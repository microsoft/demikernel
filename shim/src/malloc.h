#ifndef _MALLOC_H_
#define _MALLOC_H_

extern void init_malloc_reent_guards();
extern int is_reentrant_malloc_call();

#endif // _MALLOC_H_