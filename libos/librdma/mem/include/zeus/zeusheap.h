// -*- C++ -*-

/*

  The Hoard Multiprocessor Memory Allocator
  www.hoard.org

  Author: Emery Berger, http://www.emeryberger.com
 
  Copyright (c) 1998-2018 Emery Berger
  
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

#ifndef ZEUS_ZEUSHEAP_H
#define ZEUS_ZEUSHEAP_H

#include <assert.h>

#include "heaplayers.h"

using namespace HL;

// The minimum allocation grain for a given object -
// that is, we carve objects out of chunks of this size.
#define SUPERBLOCK_SIZE 65536

// The number of 'emptiness classes'; see the ASPLOS paper for details.
#define EMPTINESS_CLASSES 8


// Zeus-specific layers

#include "thresholdheap.h"
#include "zeus/heapmanager.h"
#include "addheaderheap.h"
#include "threadpoolheap.h"
#include "redirectfree.h"
#include "ignoreinvalidfree.h"
#include "conformantheap.h"
#include "zeussuperblock.h"
#include "zeussuperblockheader.h"
#include "lockmallocheap.h"
#include "alignedsuperblockheap.h"
#include "alignedmmap.h"
#include "zeus/globalheap.h"

#include "thresholdsegheap.h"
#include "geometricsizeclass.h"

// Note from Emery Berger: I plan to eventually eliminate the use of
// the spin lock, since the right place to do locking is in an
// OS-supplied library, and platforms have substantially improved the
// efficiency of these primitives.

#if defined(_WIN32)
typedef HL::WinLockType TheLockType;
#elif defined(__APPLE__)
// NOTE: On older versions of the Mac OS, Zeus CANNOT use Posix locks,
// since they may call malloc themselves. However, as of Snow Leopard,
// that problem seems to have gone away. Nonetheless, we use Mac-specific locks.
typedef HL::MacLockType TheLockType;
#elif defined(__SVR4)
typedef HL::SpinLockType TheLockType;
#else
typedef HL::SpinLockType TheLockType;
#endif

#if defined(__clang__)
#pragma clang diagnostic push
#pragma clang diagnostic ignored "-Wunused-variable"
#endif

namespace Zeus {

    class MmapSource : public Hoard::AlignedMmap<SUPERBLOCK_SIZE, TheLockType> {};
  
    //
    // There is just one "global" heap, shared by all of the per-process heaps.
    //

    typedef Zeus::GlobalHeap<SUPERBLOCK_SIZE,
                             ZeusSuperblockHeader,
                             EMPTINESS_CLASSES,
                             MmapSource,
                             TheLockType>
    TheGlobalHeap;
  
    //
    // When a thread frees memory and causes a per-process heap to fall
    // below the emptiness threshold given in the function below, it
    // moves a (nearly or completely empty) superblock to the global heap.
    //

    class zeusThresholdFunctionClass {
    public:
        inline static bool function (unsigned int u,
                                     unsigned int a,
                                     size_t objSize)
        {
            /*
              Returns 1 iff we've crossed the emptiness threshold:
	
              U < A - 2S   &&   U < EMPTINESS_CLASSES-1/EMPTINESS_CLASSES * A
              
            */
            auto r = ((EMPTINESS_CLASSES * u) < ((EMPTINESS_CLASSES-1) * a)) && ((u < a - (2 * SUPERBLOCK_SIZE) / objSize));
            return r;
        }
    };
  

    class SmallHeap;
  
    //  typedef Zeus::ZeusSuperblockHeader<TheLockType, SUPERBLOCK_SIZE, SmallHeap> HSHeader;
    typedef ZeusSuperblock<TheLockType,
                           SUPERBLOCK_SIZE,
                           SmallHeap,
                           ZeusSuperblockHeader> SmallSuperblockType;

    //
    // The heap that manages small objects.
    //
    class SmallHeap : 
        public Hoard::ConformantHeap<
        ZeusManager<Hoard::AlignedSuperblockHeap<TheLockType, SUPERBLOCK_SIZE, MmapSource>,
                    TheGlobalHeap,
                    SmallSuperblockType,
                    EMPTINESS_CLASSES,
                    TheLockType,
                    zeusThresholdFunctionClass,
                    SmallHeap> > 
    {};

    class BigHeap;

    typedef ZeusSuperblock<TheLockType,
                           SUPERBLOCK_SIZE,
                           BigHeap,
                           ZeusSuperblockHeader>
    BigSuperblockType;

    // The heap that manages large objects.

#if 0

    // Old version: slow and now deprecated. Returns every large object
    // back to the system immediately.
    typedef ConformantHeap<HL::LockedHeap<TheLockType,
                                          Hoard::AddHeaderHeap<BigSuperblockType,
                                                        SUPERBLOCK_SIZE,
                                                        MmapSource > > >
    bigHeapType;

#else

    // Experimental faster support for large objects.  MUCH MUCH faster
    // than the above (around 400x in some tests).  Keeps the amount of
    // retained memory at no more than X% more than currently allocated.

    class objectSource : public Hoard::AddHeaderHeap<BigSuperblockType,
                                              SUPERBLOCK_SIZE,
                                              MmapSource> {};

    typedef HL::ThreadHeap<64, HL::LockedHeap<TheLockType,
                                              Hoard::ThresholdSegHeap<25,      // % waste
                                                                      1048576, // at least 1MB in any heap
                                                                      80,      // num size classes
                                                                      Hoard::GeometricSizeClass<20>::size2class,
                                                                      Hoard::GeometricSizeClass<20>::class2size,
                                                                      Hoard::GeometricSizeClass<20>::MaxObjectSize,
                                                                      AdaptHeap<DLList, objectSource>,
                                                                      objectSource> > >
    bigHeapType;
#endif

    class BigHeap : public bigHeapType {};

    enum { BigObjectSize = 
           HL::bins<SmallSuperblockType::Header, SUPERBLOCK_SIZE>::BIG_OBJECT };

    //
    // Each thread has its own heap for small objects.
    //
    class PerThreadZeusHeap :
        public Hoard::RedirectFree<Hoard::LockMallocHeap<SmallHeap>,
                                   SmallSuperblockType> {
    private:
        void nothing() {
            _dummy[0] = _dummy[0];
        }
        // Avoid false sharing.
        char _dummy[64];
    };
  

    template <int N, int NH>
    class ZeusHeap :
        public HL::ANSIWrapper<
        Hoard::IgnoreInvalidFree<
            HL::HybridHeap<Zeus::BigObjectSize,
                           Hoard::ThreadPoolHeap<N, NH, Zeus::PerThreadZeusHeap>,
                           Zeus::BigHeap> > >
    {
    public:
    
        enum { BIG_OBJECT = Zeus::BigObjectSize };
    
        ZeusHeap() {
            enum { BIG_HEADERS = sizeof(Zeus::BigSuperblockType::Header),
                   SMALL_HEADERS = sizeof(Zeus::SmallSuperblockType::Header)};
            static_assert(BIG_HEADERS == SMALL_HEADERS,
                          "Headers must be the same size.");
        }
    };

}

#if defined(__clang__)
#pragma clang diagnostic pop
#endif

#endif
