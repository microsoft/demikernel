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
#include "queue.h"
#include <list>
#include <unordered_map>
#include <thread>
#include <assert.h>
#include <unistd.h>

#define BUFFER_SIZE 1024
#define MAGIC 0x10102010
#define PUSH_MASK 0x1
#define TOKEN_MASK 0xFF00000
#define QUEUE_MASK 0xFFFF0000
#define IS_PUSH(t) t & PUSH_MASK
// qtoken format
// | 16 bits = hash(thread_id) | 47 bits = token | 1 bit = push or pop |

namespace Zeus {

thread_local static int64_t queue_counter = 0;
thread_local static int64_t token_counter = 0;
thread_local static std::hash<std::thread::id> hasher;
thread_local static uint64_t hash;
    
template <class QueueType>
class QueueLibrary
{
    std::unordered_map<int, QueueType> queues;
    std::unordered_map<qtoken, int> pending;
    
public:

    QueueLibrary() {
        hash = hasher(std::this_thread::get_id());
        queue_counter = hash & QUEUE_MASK;
        token_counter = hash & TOKEN_MASK;
    };

    // ================================================
    // Queue Management functions
    // ================================================

    int HasQueue(int qd) {
        int ret = (queues.find(qd) != queues.end());
        return ret;
    };

    QueueType& GetQueue(int qd) {
        assert(HasQueue(qd));
        return queues.at(qd);
    };
    
    qtoken GetNewToken(int qd, bool isPush) {
        qtoken t = (token_counter == -2) ?
            // skip the range including -1 and 0
            (token_counter += 4) :
            (token_counter += 2);
        if (isPush) t |= PUSH_MASK;
        //printf("GetNewTokan qd:%d\n", qd);
        assert((t & TOKEN_MASK) == (hash & TOKEN_MASK));
        pending[t] = qd;
        return t;
    };

    QueueType& NewQueue(BasicQueueType type) {
        int qd = queue_counter++;
        queues[qd] = new QueueType(type, qd);
        return queues[qd];
    };

    void InsertQueue(QueueType q) {
        printf("library.h/InsertQueue() qd: %d\n", q.GetQD());
        assert(queues.find(q.GetQD()) == queues.end());
        queues[q.GetQD()] = q;
    };


    void RemoveQueue(int qd) {
        assert(queues.find(qd) == queues.end());
        queues.erase(qd);    
    };

    // ================================================
    // Generic interfaces to libOS syscalls
    // ================================================

    int queue(int domain, int type, int protocol) {
        int qd = QueueType::queue(domain, type, protocol);
        if (qd > 0)
            InsertQueue(QueueType(NETWORK_Q, qd));
        return qd;
    };

    int bind(int qd, struct sockaddr *saddr, socklen_t size) {
        QueueType &q = GetQueue(qd);
        return q.bind(saddr, size);
    };

    int accept(int qd, struct sockaddr *saddr, socklen_t *size) {
        QueueType &q = GetQueue(qd);
        int newqd = q.accept(saddr, size);
        if (newqd != -1){
            printf("will InsertQueue for newqd:%d\n", newqd);
            InsertQueue(QueueType(NETWORK_Q, newqd));
        }
        return newqd;
    };

    int listen(int qd, int backlog) {
        QueueType &q = GetQueue(qd);
        return q.listen(backlog);
    };

    int connect(int qd, struct sockaddr *saddr, socklen_t size) {
        QueueType &q = GetQueue(qd);
        int newqd = q.connect(saddr, size);
        if (newqd > 0)
            InsertQueue(QueueType(NETWORK_Q, newqd));
        return newqd;
    };

    int open(const char *pathname, int flags) {
        // use the fd as qd
        int qd = QueueType::open(pathname, flags);
        if (qd > 0)
            InsertQueue(QueueType(FILE_Q, qd));
        return qd;
    };

    int open(const char *pathname, int flags, mode_t mode) {
        // use the fd as qd
        int qd = QueueType::open(pathname, flags, mode);
        if (qd > 0)
            InsertQueue(QueueType(FILE_Q, qd));
        return qd;
    };

    int creat(const char *pathname, mode_t mode) {
        // use the fd as qd
        int qd = QueueType::creat(pathname, mode);
        if (qd > 0)
            InsertQueue(QueueType(FILE_Q, qd));
        return qd;
    };
    
    int close(int qd) {
        if (!HasQueue(qd))
            return -1;
        
        QueueType &queue = GetQueue(qd);
        int res = queue.close();
        RemoveQueue(qd);    
        return res;
    };

    int qd2fd(int qd) {
        if (!HasQueue(qd))
            return -1;
        QueueType &q = GetQueue(qd);
        return q.fd();
    };
    
    qtoken push(int qd, struct Zeus::sgarray &sga) {
        if (!HasQueue(qd))
            return -1;
        
        QueueType &queue = GetQueue(qd);
        if (queue.GetType() == FILE_Q)
            // pushing to files not implemented yet
            return -1;

        qtoken t = GetNewToken(qd, true);
        ssize_t res = queue.push(t, sga);
        // if push returns 0, then the sga is enqueued, but not pushed
        if (res == 0) {
            return t;
        } else {
            // if push returns something else, then sga has been
            // successfully pushed
            return 0;
        }
    };

    qtoken pop(int qd, struct Zeus::sgarray &sga) {
        if (!HasQueue(qd))
            return -1;
        
        QueueType &queue = GetQueue(qd);
        if (queue.GetType() == FILE_Q)
            // popping from files not implemented yet
            return -1;

        qtoken t = GetNewToken(qd, false);
        ssize_t res = queue.pop(t, sga);
        if (res == 0) {
            return t;
        } else {
            // if push returns something else, then sga has been
            // successfully popped and result is in sga
            return 0;
        }
    };

    ssize_t light_pop(int qd, struct Zeus::sgarray &sga) {
        //printf("call light_pop\n");
        if (!HasQueue(qd))
            return -1;
        
        QueueType &queue = GetQueue(qd);
        if (queue.GetType() == FILE_Q)
            // popping from files not implemented yet
            return -1;

        qtoken t = GetNewToken(qd, false);
        ssize_t res = queue.light_pop(t, sga);
        return res;
    };

    ssize_t wait(qtoken qt, struct sgarray &sga) {
        auto it = pending.find(qt);
        assert(it != pending.end());
        int qd = it->second;
        assert(HasQueue(qd));

        QueueType &queue = GetQueue(qd);
        if (queue.GetType() == FILE_Q)
            // waiting on files not implemented yet
            return 0;

        return queue.wait(qt, sga); 
    }

    ssize_t wait_any(qtoken *qts,
                     size_t num_qts,
                     struct sgarray &sga) {
        ssize_t res = 0;
        QueueType *qs[num_qts];
        for (unsigned int i = 0; i < num_qts; i++) {
            auto it = pending.find(qts[i]);
            assert(it != pending.end());
            auto it2 = queues.find(it->second);
            qs[i] = &it2->second;
        }
        
        while (res == 0) {
            for (unsigned int i = 0; i < num_qts; i++) {
                res = qs[i]->poll(qts[i], sga);
                if (res != 0) break;
            }
        }

        return res;
    };
            
    ssize_t wait_all(qtoken *qts,
                     size_t num_qts,
                     struct sgarray *sgas) {
        ssize_t res = 0;
        for (unsigned int i = 0; i < num_qts; i++) {
            auto it = pending.find(qts[i]);
            assert(it != pending.end());
            QueueType &q = GetQueue(it->second);
            ssize_t r = q.wait(qts[i], sgas[i]);
            if (r > 0) res += r;
        }
        return res;
    }

    ssize_t blocking_push(int qd,
                          struct sgarray &sga) {
        if (!HasQueue(qd))
            return -1;
        
        QueueType &queue = GetQueue(qd);
        if (queue.GetType() == FILE_Q)
            // popping from files not implemented yet
            return -1;

        qtoken t = GetNewToken(qd, false);
        ssize_t res = queue.push(t, sga);
        if (res == 0) {
            return wait(t, sga);
        } else {
            // if push returns something else, then sga has been
            // successfully popped and result is in sga
            return res;
        }
    }

    ssize_t blocking_pop(int qd,
                         struct sgarray &sga) {
        if (!HasQueue(qd))
            return -1;
        
        QueueType &queue = GetQueue(qd);
        if (queue.GetType() == FILE_Q)
            // popping from files not implemented yet
            return -1;

        qtoken t = GetNewToken(qd, false);
        ssize_t res = queue.pop(t, sga);
        if (res == 0) {
            return wait(t, sga);
        } else {
            // if push returns something else, then sga has been
            // successfully popped and result is in sga
            return res;
        }
    }
    
    int merge(int qd1, int qd2) {
        if (!HasQueue(qd1) || !HasQueue(qd2))
            return -1;

        return 0;
    };

    int filter(int qd, bool (*filter)(struct sgarray &sga)) {
        if (!HasQueue(qd))
            return -1;

        return 0;
    };

};

} // namespace Zeus
#endif /* _COMMON_LIBRARY_H_ */
