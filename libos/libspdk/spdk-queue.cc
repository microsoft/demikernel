// -*- mode: c++; c-file-style: "k&r"; c-basic-offset: 4 -*-
/***********************************************************************
 *
 * libos/spdk/spdk-queue.cc
 *   POSIX implementation of Zeus queue interface
 *
 * Copyright 2018 Irene Zhang  <irene.zhang@microsoft.com>
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

#include "spdk-queue.h"
#include "common/library.h"
// hoard include
#include "libzeus.h"
#include <fcntl.h>
#include <unistd.h>
#include <arpa/inet.h>
#include <netinet/tcp.h>
#include <assert.h>
#include <string.h>
#include <errno.h>
#include <sys/uio.h>
#include<mutex>

extern "C" {
#include "spdk/env.h"
#include "spdk/event.h"
#include "spdk/blob.h"
#include "spdk/blobfs.h"
#include "spdk/blob_bdev.h"
#include "spdk/log.h"
#include "spdk/io_channel.h"
#include "spdk/bdev.h"
} // END of extern "C"



namespace Zeus {
namespace SPDK {

struct SPDKContext {
	std::unordered_map<spdk_blob_id, struct spdk_blob*> opened_blobs;
	std::unordered_map<spdk_blob_id, int> blob_status;
	SPDKContext(void);
	struct spdk_bdev *bdev;
	struct spdk_blob_store *app_bs;     // bs obj for the application
	struct spdk_io_channel *app_channel;
	struct spdk_bs_dev *bs_dev;
	uint64_t page_size;
	int rc;
};

struct spdk_app_opts libos_spdk_opts = {0};
static struct SPDKContext *libos_spdk_context = NULL;
static char spdk_conf_name[] = "libos_spdk.conf";
static char spdk_app_name[] = "libos_spdk";
static std::mutex spdk_init_lock;

static void
unload_complete(void *cb_arg, int bserrno)
{
    struct SPDKContext *spdk_context_t = (struct SPDKContext *)cb_arg;
    SPDK_NOTICELOG("entry\n");
    if (bserrno) {
        SPDK_ERRLOG("Error %d unloading the bobstore\n", bserrno);
        spdk_context_t->rc = bserrno;
    }
    spdk_app_stop(spdk_context_t->rc);
}


/*
 * Unload the blobstore, cleaning up as needed.
 */
static void
unload_bs(struct SPDKContext *spdk_context_t, char *msg, int bserrno)
{
    if (bserrno) {
        SPDK_ERRLOG("%s (err %d)\n", msg, bserrno);
        spdk_context_t->rc = bserrno;
    }
    if (spdk_context_t->app_bs) {
        if (spdk_context_t->app_channel) {
            spdk_bs_free_io_channel(spdk_context_t->app_channel);
        }
        spdk_bs_unload(spdk_context_t->app_bs, unload_complete, spdk_context_t);
    } else {
        spdk_app_stop(bserrno);
    }
}

static void
create_blob(struct SPDKContext *spdk_context_t)
{
    SPDK_NOTICELOG("entry\n");
    //spdk_bs_create_blob(spdk_context_t->app_bs, NULL, NULL);
    //
}

static void
spdk_bs_init_complete(void *cb_arg, struct spdk_blob_store *bs,
         int bserrno)
{
    struct SPDKContext *spdk_context_t = (struct SPDKContext *) cb_arg;

    SPDK_NOTICELOG("entry\n");
    if (bserrno) {
        char err_msg[32] = "Error init the blobstore";
        unload_bs(spdk_context_t, err_msg, bserrno);
        return;
    }

    spdk_context_t->app_bs = bs;
    SPDK_NOTICELOG("blobstore: %p\n", spdk_context_t->app_bs);
    /*
     * We will use the page size in allocating buffers, etc., later
     * so we'll just save it in out context buffer here.
     */
    spdk_context_t->page_size = spdk_bs_get_page_size(spdk_context_t->app_bs);

    /*
     * The blostore has been initialized, let's create a blob.
     * Note that we could allcoate an SPDK event and use
     * spdk_event_call() to schedule it if we wanted to keep
     * our events as limited as possible wrt the amount of
     * work that they do.
     */
    create_blob(spdk_context_t);
}



static void libos_spdk_app_start(void *arg1, void *arg2){
    struct SPDKContext *spdk_context_t = (struct SPDKContext *) arg1;
    spdk_context_t->bdev = spdk_bdev_get_by_name("Malloc0");
    if(spdk_context_t->bdev == NULL){
        spdk_app_stop(-1);
        return;
    }
    spdk_context_t->bs_dev = spdk_bdev_create_bs_dev(spdk_context_t->bdev, NULL, NULL);
	if (spdk_context_t->bs_dev == NULL) {
        SPDK_ERRLOG("Could not create blob bdev!!\n");
        spdk_app_stop(-1);
        return;
    }
    spdk_bs_init(spdk_context_t->bs_dev, NULL, spdk_bs_init_complete, spdk_context_t);
}

int libos_spdk_init(void){
    int rc = 0;
    spdk_init_lock.try_lock();
    assert(!libos_spdk_context);
    libos_spdk_context = (struct SPDKContext *) calloc(1, sizeof(struct SPDKContext));
	if(libos_spdk_context == NULL){
        // haddle Error Here
        spdk_app_fini();
        spdk_init_lock.unlock();
        return -1;
	}
    spdk_app_opts_init(&libos_spdk_opts);
    libos_spdk_opts.name = spdk_app_name;
    libos_spdk_opts.config_file = spdk_conf_name;
    spdk_init_lock.unlock();
    rc = spdk_app_start(&libos_spdk_opts, libos_spdk_app_start, libos_spdk_context, NULL);
    return rc;
}


int
SPDKQueue::socket(int domain, int type, int protocol)
{
    spdk_log_open();
    printf("SPDKQueue:socket()\n");
    spdk_log_close();
    return 0;
}

int
SPDKQueue::bind(struct sockaddr *saddr, socklen_t size)
{
    return 0;
}

int
SPDKQueue::accept(struct sockaddr *saddr, socklen_t *size)
{
    return 0;
}

int
SPDKQueue::listen(int backlog)
{
    return 0;
}


int
SPDKQueue::connect(struct sockaddr *saddr, socklen_t size)
{
    return 0;
}

int
SPDKQueue::open(const char *pathname, int flags)
{
    // use the fd as qd
    //int qd = ::open(pathname, flags);
    if(!libos_spdk_context){
        libos_spdk_init();
    }else{
        create_blob(libos_spdk_context);
    }
    return 0;
}

int
SPDKQueue::open(const char *pathname, int flags, mode_t mode)
{
    // use the fd as qd
    //int qd = ::open(pathname, flags, mode);
    return 0;
}

int
SPDKQueue::creat(const char *pathname, mode_t mode)
{
    // use the fd as qd
    //int qd = ::creat(pathname, mode);
    return 0;
}

int
SPDKQueue::close()
{
    //return ::close(qd);
    int ret = 0;
    char err_msg[32]="";
    unload_bs(libos_spdk_context, err_msg, 0);
    return ret;
}

int
SPDKQueue::fd()
{
    return qd;
}

void
SPDKQueue::ProcessIncoming(PendingRequest &req)
{
    return;
}
 
void
SPDKQueue::ProcessOutgoing(PendingRequest &req)
{
    return;
}
 
void
SPDKQueue::ProcessQ(size_t maxRequests)
{
    return;
}


ssize_t
SPDKQueue::Enqueue(qtoken qt, struct sgarray &sga)
{
    return 0;
}

ssize_t
SPDKQueue::push(qtoken qt, struct sgarray &sga)
{
    return 0;
}

ssize_t
SPDKQueue::pop(qtoken qt, struct sgarray &sga)
{
    return 0;
}

ssize_t
SPDKQueue::peek(qtoken qt, struct sgarray &sga)
{
    return 0;
}

ssize_t
SPDKQueue::wait(qtoken qt, struct sgarray &sga)
{
    return 0;
}

ssize_t
SPDKQueue::poll(qtoken qt, struct sgarray &sga)
{
    return 0;
}


} // namespace SPDK
} // namespace Zeus
