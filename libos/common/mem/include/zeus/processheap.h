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

#ifndef ZEUS_PROCESSHEAP_H
#define ZEUS_PROCESSHEAP_H

#include <cstdlib>

#include "alignedsuperblockheap.h"
#include "conformantheap.h"
#include "hoard/emptyhoardmanager.h"
#include "zeus/zeusmanager.h"
#include "zeus/zeussuperblock.h"

namespace Zeus {

    template <size_t SuperblockSize,
              template <class LockType_,
                        int SuperblockSize_,
                        typename HeapType_> class Header_,
              int EmptinessClasses,
              class LockType,
              class ThresholdClass,
              class MmapSource>
    class ProcessHeap :
        public Hoard::ConformantHeap<
            ZeusManager<Hoard::AlignedSuperblockHeap<LockType,
                                               SuperblockSize,
                                               MmapSource>,
                        Hoard::EmptyHoardManager<ZeusSuperblock<LockType,
                                                                    SuperblockSize,
                                                                    Zeus::ProcessHeap<SuperblockSize,
                                                                                      Header_,
                                                                                      EmptinessClasses,
                                                                                      LockType,
                                                                                      ThresholdClass,
                                                                                      MmapSource>,
                                                                    Header_>>,
                        ZeusSuperblock<LockType,
                                       SuperblockSize,
                                       Zeus::ProcessHeap<SuperblockSize,
                                                         Header_,
                                                         EmptinessClasses,
                                                         LockType,
                                                         ThresholdClass,
                                                         MmapSource>, Header_>,
                        EmptinessClasses,
                        LockType,
                        ThresholdClass,
                        Zeus::ProcessHeap<SuperblockSize,
                                          Header_,
                                          EmptinessClasses,
                                          LockType,
                                          ThresholdClass,
                                          MmapSource> > > {
  
    public:
  
        ProcessHeap (void) {}
    
        // Disable allocation from this heap.
        inline void * malloc (size_t);

    private:

        // Prevent copying or assignment.
        ProcessHeap (const ProcessHeap&);
        ProcessHeap& operator=(const ProcessHeap&);

    };

} // Zeus

#endif
