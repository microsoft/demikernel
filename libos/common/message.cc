// -*- mode: c++; c-file-style: "k&r"; c-basic-offset: 4 -*-
/***********************************************************************
 *
 * message.cc:
 *   logging functions
 *
 * Copyright 2013 Dan R. K. Ports  <drkp@cs.washington.edu>
 * Copyright 2009-2012 Massachusetts Institute of Technology
 *
 * Permission is hereby granted, free of charge, to any person
 * obtaining a copy of this software and associated documentation
 * files (the "Software"), to deal in the Software without
 * restriction, including without limitation the rights to use, copy,
 * modify, merge, publish, distribute, sublicense, and/or sell copies
 * of the Software, and to permit persons to whom the Software is
 * furnished to do so, subject to the following conditions:
 *
 * The above copyright notice and this permission notice shall be
 * included in all copies or substantial portions of the Software.
 *
 * THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND,
 * EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
 * MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND
 * NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS
 * BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN
 * ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN
 * CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
 * SOFTWARE.
 *
 **********************************************************************/

#include "message.h"

#include <ctype.h>
#include <errno.h>
#include <fnmatch.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <unistd.h>
#include <sys/time.h>

#include <mutex>

#define BACKTRACE_ON_PANIC 1
#if BACKTRACE_ON_PANIC
#include <execinfo.h>
#endif

#define TIMESTAMP_BASE62 0
#define TIMESTAMP_NUMERIC 1

std::mutex message_mtx;

void __attribute__((weak))
Message_VA(enum Message_Type type,
           const char *fname, int line, const char *func,
           const char *fmt, va_list args)
{
    _Message_VA(type, stderr, fname, line, func, fmt, args);
}

void
_Message(enum Message_Type type,
         const char *fname, int line, const char *func,
         const char *fmt, ...)
{
    va_list args;
    va_start(args, fmt);
    Message_VA(type, fname, line, func, fmt, args);
    va_end(args);

    Message_DoFrees();
}

void
_Message_VA(enum Message_Type type, FILE *fp,
            const char *fname, int line, const char *func,
            const char *fmt, va_list args)
{
    // Lock mutex to make sure the output is not mangled.
    message_mtx.lock();

    static int haveColor = -1;
    struct msg_desc {
        const char *prefix;
        const char *color;
    };
    const msg_desc descs[MSG_NUM_TYPES+1] = {
        {"PANIC", "1;31"},
        {"!",     "1;33"},
        {"*",     0},
        {" ",     "22;37"},
        {"<Invalid message type>", 0},
    };

    if (haveColor == -1)
        haveColor = isatty(fileno(fp));

    int nDesc = type & (~MSG_PERROR);
    if (nDesc > MSG_NUM_TYPES)
        nDesc = MSG_NUM_TYPES;

    if (haveColor && descs[nDesc].color)
        fprintf(fp, "\033[%sm", descs[nDesc].color);

#if TIMESTAMP_NUMERIC || TIMESTAMP_BASE62
    struct timeval tv;
    if (gettimeofday(&tv, NULL) >= 0) {
        if (TIMESTAMP_NUMERIC) {
            struct tm *tm = localtime(&tv.tv_sec);
            fprintf(fp,
                    "%04d%02d%02d-%02d%02d%02d-%04d %05d ",
                    1900+tm->tm_year, tm->tm_mon, tm->tm_mday,
                    tm->tm_hour, tm->tm_min, tm->tm_sec,
                    (int)(tv.tv_usec / 100), getpid());
        } else if (TIMESTAMP_BASE62) {
            static const char base62[] =
                "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ"
                "abcdefghijklmnopqrstuvwxyz";
            // Convert to tenths of milliseconds.
            uint64_t val = (uint64_t)tv.tv_sec * 10000 +
                tv.tv_usec / 100;
            char buf[5];
            for (size_t i = 0; i < sizeof buf; ++i) {
                buf[sizeof buf - i - 1] =
                    base62[val % 62];
                val /= 62;
            }
            fprintf(fp, "%.*s %05d ", (int)sizeof buf, buf,
                        getpid());
        }
    }
#endif  // TIMESTAMP_NUMERIC || TIMESTAMP_BASE62

    fprintf(fp, "%s ", descs[nDesc].prefix);

    if (fname) {
        const char *fbasename = strrchr(fname, '/');
        if (fbasename)
            ++fbasename;
        else
            fbasename = fname;
        char filepos[32];
        snprintf(filepos, sizeof(filepos)/sizeof(filepos[0]),
                 "(%s:%d):", fbasename, line);
        fprintf(fp, "%-15s %-19s ",
                func, filepos);
    }

    vfprintf(fp, fmt, args);

    if (type & MSG_PERROR)
        fprintf(fp, ": %s", strerror(errno));

    if (haveColor && descs[nDesc].color)
        fputs("\033[0m", fp);
    fprintf(fp, "\n");
    fflush(fp);

    // Unlock mutex.
    message_mtx.unlock();
}

void _Panic(void)
{
#if BACKTRACE_ON_PANIC
    void *bt[100];
    size_t size = backtrace(bt, 100);
    char **strings = backtrace_symbols(bt, size);
    if (strings) {
        for (unsigned int i = 0; i < size; ++i) {
            Warning("%s", strings[i]);
        }
    }
#endif
    abort();
    exit(1);
}

#define MAX_DEFERRED_FREES 16

static void *deferredFrees[MAX_DEFERRED_FREES];
static int nDeferredFrees;

const char *
Message_DFree(char *buf)
{
    if (buf) {
        if (nDeferredFrees == MAX_DEFERRED_FREES)
            Panic("Too many deferred frees");
        deferredFrees[nDeferredFrees++] = buf;
    }
    return buf;
}

void
Message_DoFrees(void)
{
    for (int i = 0; i < nDeferredFrees; ++i)
        free(deferredFrees[i]);
    nDeferredFrees = 0;
}

bool
_Message_DebugEnabled(const char *fname)
{
    static bool parsed;
    static char *buf;
    static char **pats;
    static int nPats;
    if (!parsed) {
        parsed = true;
        const char *env = getenv("DEBUG");
        if (env && strlen(env)) {
            buf = strdup(env);
            if (!buf)
                Panic("Failed to allocate buffer");
            nPats = 1;
            for (size_t i = 0; i < strlen(buf); ++i) {
                if (buf[i] == ',' || buf[i] == ' ')
                    ++nPats;
            }
            pats = (char **)malloc(nPats * sizeof *pats);
            if (!pats)
                Panic("Failed to allocate buffer");
            pats[0] = buf;
            int patOut = 1;
            for (size_t i = 0; i < strlen(buf); ++i) {
                if (buf[i] == ',' || buf[i] == ' ') {
                    pats[patOut] = &buf[i+1];
                    buf[i] = '\0';
                }
            }
        }
    }
    if (!buf)
        return false;

    bool result = false;
    if (strcmp(pats[0], "all") == 0 || pats[0][0] == '^')
        result = true;

    const char *fbasename = strrchr(fname, '/');
    if (fbasename)
        ++fbasename;
    for (int i = 0; i < nPats; ++i) {
        char *pat = pats[i];
        if (pat[0] == '^')
            ++pat;

        if (fnmatch(pat, fname, FNM_PATHNAME) == 0 ||
            (fbasename &&
             fnmatch(pat, fbasename, FNM_PATHNAME) == 0)) {
            if (pats[i][0] == '^')
                result = false;
            else
                result = true;
        }
    }
    return result;
}

void
_Message_Hexdump(const void *data, int len)
{
    const unsigned char *d = (const unsigned char *)data;
    char buf[80];
    char *out;

    for (int base = 0; base < len; base += 16) {
        out = buf;
        sprintf(out, "%08x", base);
        out += 8;
        for (int offset = 0; offset < 16; ++offset) {
            if (offset == 8)
                *(out++) = ' ';
            if (base + offset >= len) {
                strcpy(out, "   ");
            } else {
                sprintf(out, " %02x", d[base+offset]);
            }
            out += 3;
        }
        strcpy(out, " |");
        out += 2;
        for (int offset = 0; offset < 16; ++offset) {
            if (base + offset >= len)
                break;
            else if (isprint(d[base+offset]))
                *out = d[base+offset];
            else
                *out = '.';
            ++out;
        }
        strcpy(out, "|");
        _Message(MSG_DEBUG, NULL, 0, NULL, "%s", buf);
    }
}

char *
Message_FmtBlob(const void *data, int len)
{
    static int blobmax = -1;
    if (blobmax == -1) {
        const char *env = getenv("BLOBMAX");
        if (!env) {
            blobmax = 32;
        } else {
            blobmax = atoi(env);
        }
    }

    int plen = len;
    if (plen > blobmax)
        plen = blobmax;

    char *buf = (char *)malloc(plen + 3);
    if (!buf)
        return NULL;

    buf[0] = '|';
    int i;
    for (i = 0; i < plen; ++i) {
        if (isprint(((char*)data)[i]))
            buf[i+1] = ((char*)data)[i];
        else
            buf[i+1] = '.';
    }
    if (len != plen)
        buf[i+1] = '>';
    else
        buf[i+1] = '|';
    buf[i+2] = '\0';
    return buf;
}

void
PanicOnSignal(int signo)
{
    Panic("Received signal %d", signo);
}
