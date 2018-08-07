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
#include "concurrentqueue.h"

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

struct SPDKPendingRequest {
    public:
        bool isDone;
        SPDK_OP req_op;
        char *file_name;
        int file_flags;
        int new_qd;
        spdk_blob_id file_blobid;
        spdk_blob *file_blob;
        struct sgarray &sga;
        SPDKPendingRequest(SPDK_OP req_op, struct sgarray &sga) :
            isDone(false),
            req_op(req_op),
            file_name(NULL),
            file_flags(0),
            new_qd(-1),
            file_blobid(-1),
            file_blob(NULL),
            sga(sga) { };
};

struct SPDKCallbackArgs {
    qtoken qt;
    struct SPDKPendingRequest *req;
};

struct SPDKContext {
	std::unordered_map<spdk_blob_id, struct spdk_blob*> opened_blobs;
	std::unordered_map<spdk_blob_id, int> blob_status;
	struct spdk_bdev *bdev;
	struct spdk_blob_store *app_bs;     // bs obj for the application
	struct spdk_io_channel *app_channel;
	struct spdk_bs_dev *bs_dev;
	uint64_t page_size;
    uint64_t cluster_size;
	int rc;
};

struct spdk_app_opts libos_spdk_opts = {0};
static struct SPDKContext *libos_spdk_context = NULL;
static char spdk_conf_name[] = "libos_spdk.conf";
static char spdk_app_name[] = "libos_spdk";
static std::mutex spdk_init_lock;

static std::unordered_map<qtoken, SPDKPendingRequest> libos_pending_reqs;
// multi-producer, single-consumer queue
// the only consumer for this queue will be the spdk_bg_thread
// which only perform the spdk related call since they are all async
// and relise on callback function.
static moodycamel::ConcurrentQueue<qtoken> spdk_work_queue;
// This thread will be spawned through socket() call
// It is the single-consumer, which dequeue from the work queue
// and then issue the spdk request
// It works as a state machine, in the sense that every callback function
// called by spdk, will cal the running function of this thread
// and thus couuld return control to the application to go on issue
// IO request to spdk library
static pthread_t spdk_bg_thread = 0;


static void *libos_run_spdk_thread(void* thread_args);

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

/****************************************************************************/
// Call-back chain for initialization of spdk blobstore
static void
spdk_bs_init_complete(void *cb_arg, struct spdk_blob_store *bs,
         int bserrno)
{
    struct SPDKContext *spdk_context_t = (struct SPDKContext *) cb_arg;
    printf("spdk_bs_init_complete\n");

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

    spdk_context_t->cluster_size = spdk_bs_get_cluster_size(bs);
    // initialize the io channel
    spdk_context_t->app_channel = spdk_bs_alloc_io_channel(bs);

    /*
     * The blostore has been initialized, let's create a blob.
     * Note that we could allcoate an SPDK event and use
     * spdk_event_call() to schedule it if we wanted to keep
     * our events as limited as possible wrt the amount of
     * work that they do.
     */
    // After finish the call-chain, all async, and the callbacks, need to
    // return the control of this thread(spdk thread) to its loop function
    libos_run_spdk_thread(NULL);
}


static void libos_spdk_app_start(void *arg1, void *arg2){
    struct SPDKContext *spdk_context_t = (struct SPDKContext *) arg1;
    printf("libos_spdk_app_start\n");
    // TODO: here, the Malloc0 is going to be replaced by real device!
    spdk_context_t->bdev = spdk_bdev_get_by_name("Malloc0");
    if(spdk_context_t->bdev == NULL){
        printf("spdk_context_t->bdev is NULL, will do stop\n");
        spdk_app_stop(-1);
        return;
    }
    spdk_context_t->bs_dev = spdk_bdev_create_bs_dev(spdk_context_t->bdev, NULL, NULL);
	if (spdk_context_t->bs_dev == NULL) {
        printf("ERROR: could not create blob bdev\n");
        SPDK_ERRLOG("Could not create blob bdev!!\n");
        spdk_app_stop(-1);
        return;
    }
    spdk_bs_init(spdk_context_t->bs_dev, NULL, spdk_bs_init_complete, spdk_context_t);
}

int libos_spdk_init(void){
    int rc = 0;
    printf("libos_spdk_init\n");
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
    printf("libos_spdk_init will call spdk_app_start\n");
    rc = spdk_app_start(&libos_spdk_opts, libos_spdk_app_start, libos_spdk_context, NULL);
    return rc;
}
/****************************************************************************/

/****************************************************************************/
// Call back cahin for creation a new blob. (blob <-> file)
static void spdk_open_blob_resize_callback(void *cb_args, int bserrno){
    struct SPDKCallbackArgs *args = (struct SPDKCallbackArgs*)cb_args;
    uint64_t file_init_len = 0;
    spdk_blob_set_xattr(args->req->file_blob, "name", args->req->file_name, strlen(args->req->file_name) + 1);
    spdk_blob_set_xattr(args->req->file_blob, "length", &file_init_len, sizeof(file_init_len));
    printf("libos_open_create_new_file success\n");
}

static void spdk_open_blob_callback(void *cb_args, struct spdk_blob *blob, int bserrno){
    struct SPDKCallbackArgs *args = (struct SPDKCallbackArgs*)cb_args;
    args->req->file_blob = blob;
    spdk_blob_resize(blob, 1, spdk_open_blob_resize_callback, args);
}

static void spdk_open_create_callback(void *cb_arg, spdk_blob_id blobid, int bserrno){
    struct SPDKCallbackArgs *args = (struct SPDKCallbackArgs*)cb_arg;
    args->req->file_blobid = blobid;
    // TODO: check the type mismatch of int vs. uint64_t ?
    args->req->new_qd = blobid;
    spdk_bs_open_blob(libos_spdk_context->app_bs, blobid, spdk_open_blob_callback, args);
}

static void libos_spdk_open_create(qtoken qt, SPDKPendingRequest* pending_req){
    struct SPDKCallbackArgs *args = (struct SPDKCallbackArgs*) malloc(sizeof(struct SPDKCallbackArgs));
    args->qt = qt;
    args->req = pending_req;
    spdk_bs_create_blob(libos_spdk_context->app_bs, spdk_open_create_callback, args);
}
/****************************************************************************/

static void *libos_run_spdk_thread(void* thread_args){
    while(1){
        if(!libos_spdk_context){
            libos_spdk_init();
        }
        // start dequeuing and handling request here
        qtoken qt;
        while(spdk_work_queue.try_dequeue(qt)){
            auto it = libos_pending_reqs.find(qt);
            if(it == libos_pending_reqs.end()){
                fprintf(stderr, "ERROR: libos_run_spdk_thread cannot find dequeued REQ\n");
                // need to release all spdk resource here
                exit(1);
            }
            // process request
            switch(it->second.req_op){
                case LIBOS_OPEN_CREAT:
                    libos_spdk_open_create(qt, (&it->second));
                case LIBOS_OPEN_EXIST:
                    break;
                case LIBOS_PUSH:
                    break;
                case LIBOS_POP:
                    break;
                case LIBOS_IDLE:
                    fprintf(stderr, "ERROR, the request type is IDLE\n");
                    break;
                default:
                    fprintf(stderr, "ERROR, request type UNKNOWN\n");
            }
        }
        printf("libos_run_spdk_thread\n");
        usleep(100);
    }
}


int
SPDKQueue::socket(int domain, int type, int protocol)
{
    printf("SPDKQueue:socket()\n");
    // NOTE: let's try to do init() here first, assume app just call this once at the beginning
    assert(spdk_bg_thread == 0);
    spdk_bg_thread = pthread_create(&spdk_bg_thread, NULL, *libos_run_spdk_thread, NULL);
    return spdk_bg_thread;
}

int 
SPDKQueue::libos_spdk_open_existing_file(qtoken qt, const char *pathname, int flags){
    return -1;
}

int 
SPDKQueue::libos_spdk_create_file(qtoken qt, const char *pathname, int flags){
    int ret;
    printf("libos_spdk_create_file\n");
    // save this requeset
    struct sgarray sga;
    libos_pending_reqs.insert(std::make_pair(qt, SPDKPendingRequest(LIBOS_OPEN_CREAT, sga)));
    auto it = libos_pending_reqs.find(qt);
    it->second.file_name = (char*) malloc(strlen(pathname)*sizeof(char));;
    strcpy(it->second.file_name, pathname);
    it->second.file_flags = flags;
    spdk_work_queue.enqueue(qt);
    // busy wait until the queue is DONE
    printf("spdk_work_queue has enqueued\n");
    while(!it->second.isDone){
        usleep(1);
    }
    ret = it->second.new_qd;
    return ret;
}

int
SPDKQueue::open(qtoken qt, const char *pathname, int flags)
{
    // use the fd as qd
    //int qd = ::open(pathname, flags);
    // TODO: check flags here
    int ret;
    ret = libos_spdk_open_existing_file(qt, pathname, flags);
    if(ret < 0){
        libos_spdk_create_file(qt, pathname, flags);
    }
    return ret;
}

int
SPDKQueue::open(const char *pathname, int flags){
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

/************************ un-funcational api for spdk ************************/
int SPDKQueue::bind(struct sockaddr *saddr, socklen_t size) {return -1;}
int SPDKQueue::accept(struct sockaddr *saddr, socklen_t *size) {return -1;}
int SPDKQueue::listen(int backlog) {return -1;}
int SPDKQueue::connect(struct sockaddr *saddr, socklen_t size) {return -1;}
/****************************************************************************/


} // namespace SPDK
} // namespace Zeus
