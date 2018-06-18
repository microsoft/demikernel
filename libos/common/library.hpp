// -*- mode: c++; c-file-style: "k&r"; c-basic-offset: 4 -*-
/***********************************************************************
 *
 * libos/common/library.hpp
 *   Generic library implementation
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

#include "library.h"
#include "queue.h"
#include <thread>

namespace Zeus {

thread_local static uint64_t queue_counter = 0;
thread_local static uint64_t token_counter = 0;

template<class QueueType>
QueueLibrary<QueueType>::QueueLibrary()
{
    uint64_t hash = std::hash<std::thread::id>(std::this_thread::get_id());
    queue_counter = hash << 32;
    token_counter = hash << 48;
}

// ================================================
// Queue Management functions
// ================================================

template<class QueueType>
inline bool
QueueLibrary<QueueType>::HasQueue(int qd)
{
    return queues.find(qd) != queues.end();
}

template<class QueueType>
inline QueueType&
QueueLibrary<QueueType>::GetQueue(int qd)
{
    return queues[qd];
}

template<class QueueType>
inline qtoken
QueueLibrary<QueueType>::GetNewToken(int qd)
{
    qtoken t = token_counter++;
    pending[t] = qd;
    return t;
}

template<class QueueType>
inline QueueType&
QueueLibrary<QueueType>::NewQueue(BasicQueueType type)
{
    int qd = queue_counter++;
    queues[qd] = QueueType(type, qd);
    return queues[qd];
}

template<class QueueType>
inline void
QueueLibrary<QueueType>::InsertQueue(QueueType queue)
{
    assert(queues.find(queue.qd) == queues.end());
    queues[queue.qd] = queue;
}


template<class QueueType>
inline void
QueueLibrary<QueueType>::RemoveQueue(int qd)
{
    assert(queues.find(queue.qd) == queues.end());
    queues.erase(qd);    
}

// ================================================
// Generic interfaces to libOS syscalls
// ================================================

// create network queue
template<class QueueType>
inline int
QueueLibrary<QueueType>::queue(int domain, int type, int protocol)
{
    int qd = QueueType::queue(domain, type, protocol);
    if (qd > 0)
        InsertQueue(QueueType(NETWORK_Q, qd));
    return qd;
}

template<class QueueType>
inline int
QueueLibrary<QueueType>::bind(int qd, struct sockaddr *saddr, socklen_t size)
{
    return queues[qd].bind(saddr, size);
}

template<class QueueType>
inline int
QueueLibrary<QueueType>::accept(int qd, struct sockaddr *saddr, socklen_t *size)
{
    return queues[qd].accept(saddr, size);
}

template<class QueueType>
inline int
QueueLibrary<QueueType>::listen(int qd, int backlog)
{
    return queues[qd].listen(backlog);
}
        

template<class QueueType>
inline int
QueueLibrary<QueueType>::connect(int qd, struct sockaddr *saddr, socklen_t size)
{
    return queues[qd].connect(saddr, size);
}

template<class QueueType>
inline int
QueueLibrary<QueueType>::open(const char *pathname, int flags)
{
    // use the fd as qd
    int qd = QueueType::open(pathname, flags);
    if (qd > 0)
        InsertQueue(QueueType(FILE_Q, qd));
    return qd;
}

template<class QueueType>
inline int
QueueLibrary<QueueType>::open(const char *pathname, int flags, mode_t mode)
{
    // use the fd as qd
    int qd = QueueType::open(pathname, flags, mode);
    if (qd > 0)
        InsertQueue(QueueType(FILE_Q, qd));
    return qd;
}

template<class QueueType>
inline int
QueueLibrary<QueueType>::creat(const char *pathname, mode_t mode)
{
    // use the fd as qd
    int qd = QueueType::creat(pathname, mode);
    if (qd > 0)
        InsertQueue(QueueType(FILE_Q, qd));
    return qd;
}
    
template<class QueueType>
inline int
QueueLibrary<QueueType>::close(int qd)
{
    if (!HasQueue(qd))
        return -1;

    QueueType &queue = GetQueue(qd);
    int res = queue.close();
    RemoveQueue(qd);    
    return res;
}

template<class QueueType>
inline int
QueueLibrary<QueueType>::qd2fd(int qd) {
    if (!HasQueue(qd))
        return -1;
    return queues[qd].fd();
}
    
// TODO: Actually buffer somewhere
template<class QueueType>
inline qtoken
QueueLibrary<QueueType>::push(int qd, struct Zeus::sgarray &sga)
{
    if (!HasQueue(qd))
        return -1;

    QueueType &queue = GetQueue(qd);
    if (queue.type == FILE_Q)
        // pushing to files not implemented yet
        return 0;
   
    queue.Push(sga);
    return GetNewToken(qd);
}

template<class QueueType>
inline qtoken
QueueLibrary<QueueType>::pop(int qd, struct Zeus::sgarray &sga)
{
 
    if (!HasQueue(qd))
        return -1;
    
    QueueType &queue = GetQueue(qd);
    if (queue.type == FILE_Q)
        // pushing to files not implemented yet
        return 0;

    queue.Receive(sga);
    return GetNewToken(qd);
}

template<class QueueType>
inline ssize_t
QueueLibrary<QueueType>::wait_any(qtoken qts[], size_t num_qts)
{
    return 0;
}

template<class QueueType>
inline ssize_t
QueueLibrary<QueueType>::wait_all(qtoken qts[], size_t num_qts)
{
    return 0;
}

template<class QueueType>
inline int
QueueLibrary<QueueType>::merge(int qd1, int qd2)
{
    if (!HasQueue(qd1) || !HasQueue(qd2))
        return -1;

    return 0;
}

template<class QueueType>
inline int
QueueLibrary<QueueType>::filter(int qd, bool (*filter)(struct sgarray &sga))
{
    if (!HasQueue(qd))
        return -1;

    return 0;
}
  
} // namespace Zeus
