/*
 * lwip-queue.h
 *
 *  Created on: Jun 11, 2018
 *      Author: amanda
 */

#ifndef LIBOS_LIBLWIP_LWIP_QUEUE_H_
#define LIBOS_LIBLWIP_LWIP_QUEUE_H_

#include "include/io-queue.h"
#include "common/queue.h"
#include <list>
#include <map>
#include <netinet/in.h>

namespace Zeus {
namespace LWIP {

class LWIPQueue : public Queue {
private:
    bool is_bound = false;
    struct sockaddr_in bound_addr;

public:
    LWIPQueue() : Queue(){ };
    LWIPQueue(BasicQueueType type, int qd) :
        Queue(type, qd) {};

    // network functions
    static int queue(int domain, int type, int protocol);
    int listen(int backlog);
    int bind(struct sockaddr *saddr, socklen_t size);
    int accept(struct sockaddr *saddr, socklen_t *size);
    int connect(struct sockaddr *saddr, socklen_t size);
    int close();

    // file functions
    static int open(const char *pathname, int flags);
    static int open(const char *pathname, int flags, mode_t mode);
    static int creat(const char *pathname, mode_t mode);

    // other functions
    ssize_t push(qtoken qt, struct sgarray &sga); // if return 0, then already complete
    ssize_t pop(qtoken qt, struct sgarray &sga); // if return 0, then already ready and in sga
    ssize_t pushto(qtoken qt, struct sgarray &sga, struct sockaddr* addr); // if return 0, then already complete
    ssize_t popfrom(qtoken qt, struct sgarray &sga, struct sockaddr* addr); // if return 0, then already ready and in sga
    ssize_t wait(qtoken qt, struct sgarray &sga);
    ssize_t poll(qtoken qt, struct sgarray &sga);

    int fd();
};

} // namespace LWIP
} // namespace Zeus



#endif /* LIBOS_LIBLWIP_LWIP_QUEUE_H_ */
