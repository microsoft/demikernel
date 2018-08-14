// -*- mode: c++; c-file-style: "k&r"; c-basic-offset: 4 -*-
/***********************************************************************
 *
 * common/library.h
 *   Zeus general-purpose queue library implementation
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
#include "common/basic-queue.h"
#include <list>
#include <unordered_map>
#include <thread>
#include <assert.h>
#include <unistd.h>
#include <cstdlib>

#define BUFFER_SIZE 1024
#define MAGIC 0x10102010
#define PUSH_MASK 0x1
#define TOKEN_MASK 0x0000FFFF
#define QUEUE_MASK 0xFFFF0000
#define TOKEN(t) t & TOKEN_MASK
#define QUEUE(t) t >> 32
#define IS_PUSH(t) t & PUSH_MASK

// qtoken format
// | 32 bits = queue id | 31 bits = token | 1 bit = push or pop |

namespace Zeus {

thread_local static int64_t queue_counter = 10;
thread_local static int64_t token_counter = 10;
    
template <class NetworkQueueType, class FileQueueType>
class QueueLibrary
{
    std::unordered_map<int, Queue *> queues;
    
public:
    QueueLibrary() {
        queue_counter = rand();
        token_counter = rand();
    };

    // ================================================
    // Queue Management functions
    // ================================================

    int HasQueue(int qd) {
        int ret = (queues.find(qd) != queues.end());
        return ret;
    };

    Queue& GetQueue(int qd) {
        assert(HasQueue(qd));
        return *queues.at(qd);
    };
    
    qtoken GetNewToken(int qd, bool isPush) {
        if (token_counter == 0) token_counter++;
        qtoken t = (token_counter << 1 & TOKEN_MASK) | ((qtoken)qd << 32); 
        if (isPush) t |= PUSH_MASK;
        //printf("GetNewToken qd:%lx\n", t);
        token_counter++;
        return t;
    };

    Queue& NewQueue(QueueType type) {
        int qd = queue_counter++ & ~QUEUE_MASK;
        Queue *queue;
        switch (type) {
        case BASIC_Q:
            queue = new BasicQueue(type, qd);
            break;
        case NETWORK_Q:
            queue = new NetworkQueueType(type, qd);
            break;
        case FILE_Q:
            queue = new FileQueueType(type, qd);
            break;
        default:
            assert(0);
        }
        queues[qd] = queue;
        return *queue;
    };

    void InsertQueue(Queue *q) {
        int qd = q->GetQD();
        assert(qd == (qd & ~QUEUE_MASK));
        //printf("library.h/InsertQueue() qd: %d\n", qd);
        assert(queues.find(qd) == queues.end());
        queues[qd] = q;
    };


    void RemoveQueue(int qd) {
        auto it = queues.find(qd);
        assert(it != queues.end());
        delete it->second;
        queues.erase(it);    
    };

    // ================================================
    // Generic interfaces to libOS syscalls
    // ================================================

    int queue() {
        return NewQueue(BASIC_Q).GetQD();
    };
    
    int socket(int domain, int type, int protocol) {
        Queue &q = NewQueue(NETWORK_Q);
        int ret = q.socket(domain, type, protocol);
        if (ret < 0) {
            RemoveQueue(q.GetQD());
            return ret;
        }
        return q.GetQD();
    };

    int getsockname(int qd, struct sockaddr *saddr, socklen_t *size) {
        Queue &q = GetQueue(qd);
        return q.getsockname(saddr, size);
    };

    int bind(int qd, struct sockaddr *saddr, socklen_t size) {
        Queue &q = GetQueue(qd);
        return q.bind(saddr, size);
    };

    int accept(int qd, struct sockaddr *saddr, socklen_t *size) {
        Queue &q = GetQueue(qd);
        int newqd = q.accept(saddr, size);
        if (newqd > 0){
            //printf("will InsertQueue for newqd:%d\n", newqd);
            InsertQueue(new NetworkQueueType(NETWORK_Q, newqd));
            return newqd;
        } else if (newqd < 0) {
            return newqd;
        } else {
            return NewQueue(NETWORK_Q).GetQD();
        }
    };

    int listen(int qd, int backlog) {
        Queue &q = GetQueue(qd);
        return q.listen(backlog);
    };

    int connect(int qd, struct sockaddr *saddr, socklen_t size) {
        Queue &q = GetQueue(qd);
        int newqd = q.connect(saddr, size);
        if (newqd > 0)
            InsertQueue(new NetworkQueueType(NETWORK_Q, newqd));
        return newqd;
    };

    int open(const char *pathname, int flags) {
        Queue &q = NewQueue(FILE_Q);
        int ret = q.open(pathname, flags);
        if (ret < 0) {
            RemoveQueue(q.GetQD());
            return ret;
        } else {
            return q.GetQD();
        }
    }


    /** JINGLIU
    int open(const char *pathname, int flags) {
        // use the fd as qd
        qtoken qt = GetNewToken(LIBOS_OPEN_QD, false);
        fprintf(stderr, "Library.h: open() called\n");
        int qd = QueueType::open(qt, pathname, flags);
        fprintf(stderr, "library.h: open() returns:%d \n", qd);
        if (qd > 0){
            InsertQueue(QueueType(FILE_Q, qd));
        }
        return qd;
    };**/

    int open(const char *pathname, int flags, mode_t mode) {
        Queue &q = NewQueue(FILE_Q);
        int ret = q.open(pathname, flags, mode);
        if (ret < 0) {
            RemoveQueue(q.GetQD());
            return ret;
        } else {
            return q.GetQD();
        }
    };

    int creat(const char *pathname, mode_t mode) {
        Queue &q = NewQueue(FILE_Q);
        int ret = q.creat(pathname, mode);
        if (ret < 0) {
            RemoveQueue(q.GetQD());
            return ret;
        } else {
            return q.GetQD();
        }
    };

    int flush(int qd) {
        fprintf(stderr, "Library.h flush(%d) called", qd);
        if (!HasQueue(qd)){
            return -1;
        }
        QueueType &queue = GetQueue(qd);
        if(queue.GetType() == FILE_Q){
            qtoken t = GetNewToken(qd, true);
            int ret = queue.flush(t, false);
            fprintf(stderr, "Library.h flush returned %d\n", ret);
            return ret;
        }else{
            // flush just for storage
            return -1;
        }
    };

    
    int close(int qd) {
        if (!HasQueue(qd))
            return ZEUS_IO_ERR_NO;

        int res = -1;
        Queue &queue = GetQueue(qd);
        int res = queue.close();
        RemoveQueue(qd);    
        return res;
    };

    int qd2fd(int qd) {
        if (!HasQueue(qd))
            return ZEUS_IO_ERR_NO;
        Queue &q = GetQueue(qd);
        return q.getfd();
    };

    qtoken push(int qd, struct Zeus::sgarray &sga) {
        if (!HasQueue(qd))
            return ZEUS_IO_ERR_NO;

        Queue &queue = GetQueue(qd);
        qtoken t = GetNewToken(qd, true);
        ssize_t res;
        res = queue.push(t, sga);
        // if push returns 0, then the sga is enqueued, but not pushed
        if (res == 0) {
            return t;
        } else {
            // if push returns something else, then sga has been
            // successfully pushed
            return 0;
        }
    };

    qtoken flush_push(int qd, struct Zeus::sgarray &sga) {
        if (!HasQueue(qd))
            return -1;
        
        QueueType &queue = GetQueue(qd);
        if (queue.GetType() == FILE_Q) {
            qtoken t = GetNewToken(qd, true);
            ssize_t res = queue.flush_push(t, sga);
            if(res == 0){
                return t;
            }else{
                return 0;
            }
        }else{
            // only FILE_Q support flush_push
            return -1;
        }
    };



    qtoken pop(int qd, struct Zeus::sgarray &sga) {
        if (!HasQueue(qd))
            return ZEUS_IO_ERR_NO;
        
        Queue &queue = GetQueue(qd);
        if (queue.GetType() == FILE_Q)
            // popping from files not implemented yet
            return ZEUS_IO_ERR_NO;

        qtoken t = GetNewToken(qd, false);
        ssize_t res;
        res = queue.pop(t, sga);

        if (res > 0)
            return 0;
        else if (res == 0)
            return t;
        else
            return ZEUS_IO_ERR_NO;
    };

    ssize_t peek(int qd, struct Zeus::sgarray &sga) {
        //printf("call peekp\n");
        if (!HasQueue(qd))
            return ZEUS_IO_ERR_NO;
        
        Queue &queue = GetQueue(qd);
        int res = queue.peek(sga);

        return res;
    };

    ssize_t wait(qtoken qt, struct sgarray &sga) {
        int qd = QUEUE(qt);
        assert(HasQueue(qd));

        Queue &queue = GetQueue(qd);
        return queue.wait(qt, sga); 
    };

    ssize_t wait_any(qtoken tokens[],
                     size_t num,
                     int &offset,
                     int &qd,
                     struct sgarray &sga) {
        ssize_t res = 0;
        Queue *waitingQs[num];
        for (unsigned int i = 0; i < num; i++) {
            int qd2 = QUEUE(tokens[i]);
            auto it2 = queues.find(qd2);
            assert(it2 != queues.end());
            
            // do a quick check if something is ready
            res = it2->second->poll(tokens[i], sga);
    
            if (res != 0) {
                offset = i;
                qd = qd2;
                return res;
            }
            waitingQs[i] = it2->second;
        }
        
        while (true) {
            for (unsigned int i = 0; i < num; i++) {
                Queue *q = waitingQs[i];
                res = q->poll(tokens[i], sga);
                
                if (res != 0) {
                    offset = i;
                    qd = q->GetQD();
                    return res;
                }                    
            }
        }
    };
            
    ssize_t wait_all(qtoken tokens[],
                     size_t num,
                     struct sgarray **sgas) {
        ssize_t res = 0;
        for (unsigned int i = 0; i < num; i++) {
            Queue &q = GetQueue(QUEUE(tokens[i]));
            
            ssize_t r = q.wait(tokens[i], *sgas[i]);
            if (r > 0) res += r;
        }
        return res;
    };

    ssize_t blocking_push(int qd,
                          struct sgarray &sga) {
        if (!HasQueue(qd))
            return ZEUS_IO_ERR_NO;
        
        Queue &q = GetQueue(qd);
        if (q.GetType() == FILE_Q)
            // popping from files not implemented yet
            return ZEUS_IO_ERR_NO;

        qtoken t = GetNewToken(qd, false);
        ssize_t res = q.push(t, sga);
        if (res == 0) {
            return wait(t, sga);
        } else {
            // if push returns something else, then sga has been
            // successfully popped and result is in sga
            return res;
        }
    };

    ssize_t blocking_pop(int qd,
                         struct sgarray &sga) {
        if (!HasQueue(qd))
            return ZEUS_IO_ERR_NO;
        
        Queue &q = GetQueue(qd);
        if (q.GetType() == FILE_Q)
            // popping from files not implemented yet
            return ZEUS_IO_ERR_NO;

        qtoken t = GetNewToken(qd, false);
        ssize_t res = q.pop(t, sga);
        if (res == 0) {
            return wait(t, sga);
        } else {
            // if push returns something else, then sga has been
            // successfully popped and result is in sga
            return res;
        }
    };
    
    int merge(int qd1, int qd2) {
        if (!HasQueue(qd1) || !HasQueue(qd2))
            return ZEUS_IO_ERR_NO;

        return 0;
    };

    int filter(int qd, bool (*filter)(struct sgarray &sga)) {
        if (!HasQueue(qd))
            return ZEUS_IO_ERR_NO;

        return 0;
    };
};

} // namespace Zeus
#endif /* _COMMON_LIBRARY_H_ */
