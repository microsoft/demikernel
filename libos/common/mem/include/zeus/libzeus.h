#ifndef ZEUS_LIBZEUS_H
#define ZEUS_LIBZEUS_H

extern "C" {
// Irene: adding pin and unpin operations
void pin(void *ptr);
void unpin(void *ptr);
}
#endif
