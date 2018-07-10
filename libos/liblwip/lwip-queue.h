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
#include <unordered_map>
#include <netinet/in.h>

namespace Zeus {
namespace LWIP {

#define MAX_PKTS 4

class LWIPQueue : public Queue {
private:
    struct PendingRequest {
    public:
        bool isDone;
        ssize_t res;
        struct sgarray *sga;

        PendingRequest() :
            isDone(false),
            res(0),
            sga(NULL) { };
    };

    // queued scatter gather arrays
    std::unordered_map<qtoken, struct PendingRequest> pending;
    std::list<qtoken> workQ;

    bool is_bound = false;
    struct sockaddr_in bound_addr;

    struct sockaddr_in *default_peer_addr = NULL;
    bool has_default_peer = false;


    int bind();
    void ProcessOutgoing(PendingRequest &req);
    void ProcessIncoming(PendingRequest &req);
    void ProcessQ(size_t maxRequests);
    ssize_t Enqueue(qtoken qt, sgarray &sga);

    struct rte_mbuf *pkt_buffer[MAX_PKTS];
    int num_packets = 0;
    int pkt_idx = 0;

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
    ssize_t push(qtoken qt, struct sgarray &sga);
    ssize_t pop(qtoken qt, struct sgarray &sga);
    ssize_t wait(qtoken qt, struct sgarray &sga);
    ssize_t poll(qtoken qt, struct sgarray &sga);

    int fd();
};

int lwip_init(int argc, char* argv[]);
int lwip_init();

} // namespace LWIP
} // namespace Zeus



#endif /* LIBOS_LIBLWIP_LWIP_QUEUE_H_ */
