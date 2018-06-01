
#include "include/io-queue_c.h"

#include "include/io-queue.h"

// network functions
int zeus_queue(int domain, int type, int protocol){
    return Zeus::queue(domain, type, protocol);
}
int zeus_listen(int fd, int backlog){
    return Zeus::listen(fd, backlog);
}
int zeus_bind(int qd, struct sockaddr *saddr, socklen_t size){
    return Zeus::bind(qd, saddr, size);
}
int zeus_accept(int qd, struct sockaddr *saddr, socklen_t *size){
    return Zeus::accept(qd, saddr, size);
}
int zeus_connect(int qd, struct sockaddr *saddr, socklen_t size){
    return Zeus::connect(qd, saddr, size);
}

// eventually file functions
// int open() ..

ssize_t zeus_push(int qd, zeus_sgarry * bufs){
    Zeus::sgarray sga;
    sga.num_bufs = bufs->num_bufs;
    sga.bufs[0].buf = (Zeus::ioptr)&((bufs->bufs)[0]);
    return Zeus::push(qd, sga);
}
ssize_t zeus_pop(int qd, zeus_sgarry * bufs){
    Zeus::sgarray sga;
    sga.num_bufs = bufs->num_bufs;
    sga.bufs[0].buf = (Zeus::ioptr)&((bufs->bufs)[0]);
    return Zeus::pop(qd, sga);
}
int zeus_qd2fd(int qd){
    return Zeus::qd2fd(qd);
}


