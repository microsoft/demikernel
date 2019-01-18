#include <dmtr/fail.h>

#include <dmtr/annot.h>
#include <stdio.h>
#include <stdlib.h>

static void default_reporter(int code_arg,
      const char *expr_arg, const char *fnn_arg, const char *filen_arg,
      int lineno_arg);

static dmtr_fail_reporter_t current_reporter = &default_reporter;

void dmtr_fail_panic(const char *why_arg) {
    if (!why_arg) {
        why_arg = "*unspecified*";
    }
    /* there's really no point in checking the return code of fprintf().
     * if it fails, i don't have a backup plan for informing the
     * operator. */
    fprintf(stderr, "*** epic fail! %s\n", why_arg);
    abort();
}

void dmtr_fail_setrep(dmtr_fail_reporter_t reporter_arg) {
    if (NULL == reporter_arg) {
        current_reporter = &default_reporter;
    } else {
        current_reporter = reporter_arg;
    }
}

void dmtr_fail_report(int code_arg, const char *expr_arg,
      const char *fnn_arg, const char *filen_arg, int lineno_arg) {
   current_reporter(code_arg, expr_arg, fnn_arg, filen_arg,
         lineno_arg);
}

void default_reporter(int reply_arg, const char *expr_arg,
   const char *fnn_arg, const char *filen_arg, int lineno_arg) {
    int n = -1;

    /* to my knowledge, Windows doesn't support providing the function name,
     * so i need to tolerate a NULL value for fnn_arg. */
    if (NULL == fnn_arg) {
        n = fprintf(stderr, "FAIL %d at %s, line %d: %s\n", reply_arg,
                filen_arg, lineno_arg, expr_arg);
        if (n < 1) {
            dmtr_fail_panic("fprintf() failed.");
            DMTR_UNREACHABLE();
        }
    } else {
        n = fprintf(stderr, "FAIL %d in %s, at %s, line %d: %s\n", reply_arg,
                fnn_arg, filen_arg, lineno_arg, expr_arg);
        if (n < 1) {
            dmtr_fail_panic("fprintf() failed.");
        }
   }
}
