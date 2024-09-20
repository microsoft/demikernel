#ifndef _MALLOC_H_
#define _MALLOC_H_

extern void * (*libc_malloc)(size_t);
extern void * (*libc_calloc)(size_t, size_t);
extern void * (*libc_realloc)(void *, size_t);
extern void init_malloc_reent_guards();
extern int is_reentrant_malloc_call();
extern void init_calloc_reent_guards();
extern int is_reentrant_calloc_call();
extern void init_realloc_reent_guards();
extern int is_reentrant_realloc_call();

#endif // _MALLOC_H_