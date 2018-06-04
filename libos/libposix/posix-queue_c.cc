
#include "include/io-queue_c.h"

#include "include/io-queue.h"

// network functions
int zeus_queue(int domain, int type, int protocol){
    printf("zeus_queue\n");
    return Zeus::queue(domain, type, protocol);
}
int zeus_listen(int fd, int backlog){
    printf("zeus_listen\n");
    return Zeus::listen(fd, backlog);
}
int zeus_bind(int qd, struct sockaddr *saddr, socklen_t size){
    printf("zeus_bind\n");
    return Zeus::bind(qd, saddr, size);
}
int zeus_accept(int qd, struct sockaddr *saddr, socklen_t *size){
    printf("zeus_accept\n");
    return Zeus::accept(qd, saddr, size);
}
int zeus_connect(int qd, struct sockaddr *saddr, socklen_t size){
    printf("zeus_connect\n");
    return Zeus::connect(qd, saddr, size);
}

// eventually file functions
// int open() ..
//

void sgarray_c2cpp(zeus_sgarray * c_sga, Zeus::sgarray * sga){
    sga->num_bufs = c_sga->num_bufs;
    for(int i = 0; i < c_sga->num_bufs; i++){
         (sga->bufs[i]).len = (c_sga->bufs[i]).len;
         (sga->bufs[i]).buf = (c_sga->bufs[i]).buf;
    }
    printf("sgarray_c2cpp() return\n");
}

void sgarray_cpp2c(Zeus::sgarray * sga, zeus_sgarray * c_sga){
    c_sga->num_bufs = sga->num_bufs;
    for(int i = 0; i < sga->num_bufs; i++){
        (c_sga->bufs[i]).len = (sga->bufs[i]).len;
        (c_sga->bufs[i]).buf = (sga->bufs[i]).buf;
    }
    printf("sgarray_cpp2c() return\n");
}

ssize_t zeus_push(int qd, zeus_sgarray * bufs){
    printf("zeus_push\n");
    Zeus::sgarray sga;
    sgarray_c2cpp(bufs, &sga);
    return Zeus::push(qd, sga);
}

ssize_t zeus_pop(int qd, zeus_sgarray * bufs){
    printf("zeus_pop\n");
    Zeus::sgarray sga;
    ssize_t n = Zeus::pop(qd, sga);
    sgarray_cpp2c(&sga, bufs);
    return n;
}

int zeus_qd2fd(int qd){
    return Zeus::qd2fd(qd);
}


