// -*- mode: c++; c-file-style: "k&r"; c-basic-offset: 4 -*-
/***********************************************************************
 *
 * include/rdmasuperblockheader.h
 *   Super block header with RDMA meta data
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
 
#ifndef _RDMA_SUPERBLOCKHEADER_H_
#define _RDMA_SUPERBLOCKHEADER_H_

#include "include/hoard/hoardsuperblockheader.h"
#include <rdma/rdma_cma.h>

namespace Zeus {

    template <class LockType,
              int SuperblockSize,
              typename HeapType>
    class RdmaSuperblock;

    template <class LockType,
              int SuperblockSize,
              typename HeapType>
    class RdmaSuperblockHeader : public Hoard::HoardSuperblockHeaderExtensible<LockType,
                                                                               SuperblockSize,
                                                                               HeapType,
                                                                               RdmaSuperblock<LockType,
                                                                                              SuperblockSize,
                                                                                              HeapType>> {
    public:
        RdmaSuperblockHeader (size_t size, size_t BufferSize)
            : Hoard::HoardSuperblockHeaderExtensible<LockType,
                                                     SuperblockSize,
                                                     HeapType,
                                                     RdmaSuperblock<LockType, SuperblockSize, HeapType > >
            (size, BufferSize, (char *) (this + 1)) { };
        void setRdmaMr(struct ibv_mr *_mr) {mr = _mr;};
        struct ibv_mr* getRdmaMr() {return mr;};
        bool inRdmaWQ() {return inWQ;};
        typedef RdmaSuperblock<LockType, SuperblockSize, HeapType> SuperblockType;
    private:
        // rdma data structure for registered
        // memory region for this superbiock 
        struct ibv_mr *mr;
        // buffer currently in a work queue
        bool inWQ = false;
    };
} // namespace zeus

#endif /* _RDMA_SUPERBLOCKHEADER_H_ */
