#include <dmtr/fail.h>

#include <dmtr/annot.h>
#include <stdio.h>
#include <stdlib.h>

static void default_onfail(int error_arg,
      const char *expr_arg, const char *fnn_arg, const char *filen_arg,
      int lineno_arg);

static dmtr_onfail_t current_onfail = &default_onfail;

void dmtr_panic(const char *why_arg) {
    if (!why_arg) {
        why_arg = "*unspecified*";
    }
    /* there's really no point in checking the return code of fprintf().
     * if it fails, i don't have a backup plan for informing the
     * operator. */
    fprintf(stderr, "*** epic fail! %s\n", why_arg);
    abort();
}

void dmtr_onfail(dmtr_onfail_t onfail_arg) {
    if (NULL == onfail_arg) {
        current_onfail = &default_onfail;
    } else {
        current_onfail = onfail_arg;
    }
}

void dmtr_fail(int error_arg, const char *expr_arg,
      const char *fnn_arg, const char *filen_arg, int lineno_arg) {
   current_onfail(error_arg, expr_arg, fnn_arg, filen_arg,
         lineno_arg);
}

void default_onfail(int error_arg, const char *expr_arg,
   const char *fnn_arg, const char *filen_arg, int lineno_arg) {
    int n = -1;

    if (0 == error_arg) {
        dmtr_panic("attempt to fail with a success code.");
    }

    /* to my knowledge, Windows doesn't support providing the function name,
     * so i need to tolerate a NULL value for fnn_arg. */
    if (NULL == fnn_arg) {
        n = fprintf(stderr, "FAIL %d at %s, line %d: %s\n", error_arg,
                filen_arg, lineno_arg, expr_arg);
        if (n < 1) {
            dmtr_panic("fprintf() failed.");
            DMTR_UNREACHABLE();
        }
    } else {
        n = fprintf(stderr, "FAIL %d in %s, at %s, line %d: %s\n", error_arg,
                fnn_arg, filen_arg, lineno_arg, expr_arg);
        if (n < 1) {
            dmtr_panic("fprintf() failed.");
        }
   }
}
