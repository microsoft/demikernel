/* -*- C++ -*- */

/*
  The Hoard Multiprocessor Memory Allocator
  www.hoard.org

  Author: Emery Berger, http://www.emeryberger.org
 
  Copyright (c) 1998-2016 Emery Berger

  This program is free software; you can redistribute it and/or modify
  it under the terms of the GNU General Public License as published by
  the Free Software Foundation; either version 2 of the License, or
  (at your option) any later version.
  
  This program is distributed in the hope that it will be useful,
  but WITHOUT ANY WARRANTY; without even the implied warranty of
  MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
  GNU General Public License for more details.
  
  You should have received a copy of the GNU General Public License
  along with this program; if not, write to the Free Software
  Foundation, Inc., 59 Temple Place, Suite 330, Boston, MA  02111-1307  USA

*/

/*
 * @file   libhoard.cpp
 * @brief  This file replaces malloc etc. in your application.
 * @author Emery Berger <http://www.cs.umass.edu/~emery>
 */

#include <cstddef>
#include <new>

#include "VERSION.h"

#define versionMessage "Using the Zeus-RDMA-Hoard memory allocator (http://www.hoard.org), version " HOARD_VERSION_STRING "\n"

#include "heaplayers.h"
#include "include/zeus/libzeus.h"
#include <sys/socket.h>
#include <netinet/in.h>
#include <arpa/inet.h>

// The undef below ensures that any pthread_* calls get strong
// linkage.  Otherwise, our versions here won't replace them.  It is
// IMPERATIVE that this line appear before any files get included.

#undef __GXX_WEAK__ 

#if defined(_WIN32)
#define WIN32_LEAN_AND_MEAN

// Maximize the degree of inlining.
#pragma inline_depth(255)

// Turn inlining hints into requirements.
#define inline __forceinline
#pragma warning(disable:4273)
#pragma warning(disable: 4098)  // Library conflict.
#pragma warning(disable: 4355)  // 'this' used in base member initializer list.
#pragma warning(disable: 4074)	// initializers put in compiler reserved area.
#pragma warning(disable: 6326)  // comparison between constants.

#endif

#if HOARD_NO_LOCK_OPT
// Disable lock optimization.
volatile bool anyThreadCreated = true;
#else
// The normal case. See heaplayers/spinlock.h.
volatile bool anyThreadCreated = false;
#endif

namespace Zeus {
  
    // ZEUS_MMAP_PROTECTION_MASK defines the protection flags used for
    // freshly-allocated memory. The default case is that heap memory is
    // NOT executable, thus preventing the class of attacks that inject
    // executable code on the heap.
    // 
    // While this is not recommended, you can define HL_EXECUTABLE_HEAP as
    // 1 in heaplayers/heaplayers.h if you really need to (i.e., you're
    // doing dynamic code generation into malloc'd space).
  
#if HL_EXECUTABLE_HEAP
#define ZEUS_MMAP_PROTECTION_MASK (PROT_READ | PROT_WRITE | PROT_EXEC)
#else
#define ZEUS_MMAP_PROTECTION_MASK (PROT_READ | PROT_WRITE)
#endif

} // namespace Zeus

#include "zeustlab.h"

//
// The base Zeus heap.
//


/// Maintain a single instance of the main Zeus heap.

Zeus::ZeusHeapType * getMainZeusHeap() {
    // This function is C++ magic that ensures that the heap is
    // initialized before its first use. First, allocate a static buffer
    // to hold the heap.

    static double thBuf[sizeof(Zeus::ZeusHeapType) / sizeof(double) + 1];

    // Now initialize the heap into that buffer.
    static auto * th = new (thBuf) Zeus::ZeusHeapType;
    return th;
}

TheCustomHeapType * getCustomHeap();

enum { MAX_LOCAL_BUFFER_SIZE = 256 * 131072 };
static char initBuffer[MAX_LOCAL_BUFFER_SIZE];
static char * initBufferPtr = initBuffer;
static struct ibv_context *rdma_context = NULL;
static struct ibv_pd *rdma_globalpd = NULL;

extern bool isCustomHeapInitialized();

extern "C" {

    void * xxmalloc (size_t sz) {
        if (isCustomHeapInitialized()) {
            void * ptr = getCustomHeap()->malloc (sz);
            if (ptr == nullptr) {
                fprintf(stderr, "INTERNAL FAILURE.\n");
                abort();
            }
            return ptr;
        }
        // We still haven't initialized the heap. Satisfy this memory
        // request from the local buffer.
        void * ptr = initBufferPtr;
        initBufferPtr += sz;
        if (initBufferPtr > initBuffer + MAX_LOCAL_BUFFER_SIZE) {
            abort();
        }
        {
	  static bool initialized = false;
            if (!initialized) {
                initialized = true;
#if !defined(_WIN32)
                fprintf(stderr, versionMessage);
#endif
            }
        }
        return ptr;
    }

    void xxfree (void * ptr) {
        getCustomHeap()->free (ptr);
    }

    size_t xxmalloc_usable_size (void * ptr) {
        return getCustomHeap()->getSize (ptr);
    }

    void xxmalloc_lock() {
        // Undefined for Zeus.
    }

    void xxmalloc_unlock() {
        // Undefined for Zeus.
    }

    void pin(void * ptr) {
        getCustomHeap()->pin (ptr);
    }

    void unpin(void * ptr) {
        getCustomHeap()->unpin (ptr);
    }

  struct ibv_mr * rdma_get_mr(void * ptr, rdma_cm_id *rdma_id) {
    return getCustomHeap()->rdma_get_mr (ptr, rdma_id);
    }

    struct ibv_context* rdma_get_context() {
      assert(false);
      return rdma_context; }

    struct ibv_pd* rdma_get_pd() {
      assert(false);
      return rdma_globalpd; }
  
  void rdma_init() {
    // static bool initialized = false;
    // if (!initialized) {
    //   // struct rdma_event_channel *channel;
    //   // if ((channel = rdma_create_event_channel()) == 0) {
    //   // 	fprintf(stderr, "Could not create event channel: %s\n", strerror(errno));
    //   // 	abort();
    //   // }

    //   rdma_cm_id *rdma_id;
    //   if (rdma_create_id(NULL, &rdma_id, NULL, RDMA_PS_TCP) == 0) {
    // 	sockaddr_in dummy;
    // 	dummy.sin_family = AF_INET;
    // 	dummy.sin_port = INADDR_ANY;
    // 	dummy.sin_addr.s_addr = inet_addr("127.0.0.1");

    // 	if (rdma_resolve_addr(rdma_id, NULL, (sockaddr *)&dummy, 1) == 0) {
    // 	  struct ibv_qp_init_attr qp_attr;
    // 	  memset(&qp_attr, 0, sizeof(qp_attr));
    // 	  qp_attr.qp_type = IBV_QPT_RC;
    // 	  qp_attr.cap.max_send_wr = 1;
    // 	  qp_attr.cap.max_recv_wr = 1;
    // 	  qp_attr.cap.max_send_sge = 1;
    // 	  qp_attr.cap.max_recv_sge = 1;
    
    // 	  // set up connection queue pairs
    // 	  if (rdma_create_qp(rdma_id, NULL, &qp_attr) == 0) {
    // 	    assert(rdma_id->verbs != NULL);
    // 	    assert(rdma_id->pd != NULL);
    // 	    assert(rdma_id->verbs == rdma_id->pd->context);
    // 	    rdma_context = rdma_id->verbs;
    // 	    rdma_globalpd = rdma_id->pd;
    // 	    fprintf(stderr, "Setting up RDMA memory allocation\n");
    // 	    return;
    // 	   }
    // 	}
    //   }
      
    //   fprintf(stderr, "RDMA FAILURE: %s", strerror(errno));
    //   abort();
    // }
  }
	       
} // extern C
