// -*- mode: c++; c-file-style: "k&r"; c-basic-offset: 4 -*-
/***********************************************************************
 *
 * common/library.h
 *   Generic libos implementation
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
 
#ifndef _COMMON_LIBRARY_H_
#define _COMMON_LIBRARY_H_

#include "include/io-queue.h"

#include <list>
#include <unordered_map>

#define BUFFER_SIZE 1024
#define MAGIC 0x10102010

namespace Zeus {

template <class QueueType>
class QueueLibrary
{
    std::unordered_map<int, QueueType&> queues;
    std::unordered_map<qtoken, int> pending;
    
public:

    QueueLibrary();

    // returns true on success and false on failure
    void InsertQueue(QueueType queue);
    void RemoveQueue(int qd);
    bool HasQueue(int qd);
    QueueType& GetQueue(int qd);
    qtoken GetNewToken();

};

} // namespace Zeus
#endif /* _COMMON_LIBRARY_H_ */
