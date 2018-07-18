
#include "include/io-queue_c.h"

#include "include/io-queue.h"
#include "library.h"

#include <unistd.h>

// debug print flag
//#define __DEBUG_c_interface_cc 0


/************************************************************************/
/** translation of data structure **/
void sgarray_c2cpp(zeus_sgarray * c_sga, Zeus::sgarray * sga){
    sga->num_bufs = c_sga->num_bufs;
    for(int i = 0; i < c_sga->num_bufs; i++){
         (sga->bufs[i]).len = (c_sga->bufs[i]).len;
         (sga->bufs[i]).buf = (c_sga->bufs[i]).buf;
         (c_sga->bufs[i]).addr = (sga->bufs[i]).addr;
    }
    //printf("sgarray_c2cpp() return\n");
}

void sgarray_cpp2c(Zeus::sgarray * sga, zeus_sgarray * c_sga){
    c_sga->num_bufs = sga->num_bufs;
    for(int i = 0; i < sga->num_bufs; i++){
        (c_sga->bufs[i]).len = (sga->bufs[i]).len;
#ifdef __DEBUG_c_interface_cc
        printf("sgarray_cpp2c: i%d len:%d\n", i, (c_sga->bufs[i]).len);
#endif
        (c_sga->bufs[i]).buf = (sga->bufs[i]).buf;
        (c_sga->bufs[i]).addr = (sga->bufs[i]).addr;
    }
    //printf("sgarray_cpp2c() return\n");
}
/************************************************************************/

// network functions
int zeus_queue(int domain, int type, int protocol){
#ifdef __DEBUG_c_interface_cc
    printf("zeus_queue\n");
#endif
    return Zeus::queue(domain, type, protocol);
}

int zeus_listen(int fd, int backlog){
#ifdef __DEBUG_c_interface_cc
    printf("zeus_listen\n");
#endif
    return Zeus::listen(fd, backlog);
}

int zeus_bind(int qd, struct sockaddr *saddr, socklen_t size){
#ifdef __DEBUG_c_interface_cc
    printf("zeus_bind\n");
#endif
    return Zeus::bind(qd, saddr, size);
}

int zeus_accept(int qd, struct sockaddr *saddr, socklen_t *size){
#ifdef __DEBUG_c_interface_cc
    printf("zeus_accept\n");
#endif
    return Zeus::accept(qd, saddr, size);
}

int zeus_connect(int qd, struct sockaddr *saddr, socklen_t size){
#ifdef __DEBUG_c_interface_cc
    printf("zeus_connect\n");
#endif
    return Zeus::connect(qd, saddr, size);
}

int zeus_open(const char *pathname, int flags, mode_t mode) {
    fprintf(stderr, "NIY\n");
    return 0;
}

int zeus_creat(const char *pathname, mode_t mode) {
    fprintf(stderr, "NIY\n");
    return 0;
}

zeus_qtoken zeus_push(int qd, zeus_sgarray *sga_ptr){
#ifdef __DEBUG_c_interface_cc
    printf("zeus_push\n");
#endif
    Zeus::sgarray sga;
    sgarray_c2cpp(sga_ptr, &sga);
    zeus_qtoken n = Zeus::push(qd, sga);
    //printf("zeus_push() will return: %zd\n",n);
    return n;
}

zeus_qtoken zeus_pop(int qd, zeus_sgarray *sga_ptr){
//#ifdef __DEBUG_c_interface_cc
    //printf("zeus_pop() qd:%d\n", qd);
//#endif
    Zeus::sgarray sga;
    zeus_qtoken n = Zeus::pop(qd, sga);
    //printf("return from pop() n:%zd\n", n);
    if (n == 0){
#ifdef __DEBUG_c_interface_cc
        printf("Zeus:pop() successs\n");
#endif
        sgarray_cpp2c(&sga, sga_ptr);
    }
    return n;
}

ssize_t zeus_peek(int qd, zeus_sgarray *sga_ptr){
    Zeus::sgarray sga;
    ssize_t n = Zeus::peek(qd, sga);
    //printf("return from pop() n:%zd\n", n);
    if (n > 0){
#ifdef __DEBUG_c_interface_cc
        printf("Zeus:light_pop() successs\n");
#endif
        //sleep(10);
        sgarray_cpp2c(&sga, sga_ptr);
    }
    return n;
}

ssize_t zeus_wait(zeus_qtoken qt, zeus_sgarray *sga_ptr) {
    ssize_t ret;
    if (IS_PUSH(qt)){
        // wait for a push request
        // NOTE: assume sga_ptr for push is not in need since data has been saved with previous push()
        Zeus::sgarray sga;
        ret = Zeus::wait(qt, sga);
    }else{
        // wait for a pop request
        Zeus::sgarray sga;
        ret = Zeus::wait(qt, sga);
        if(ret >= 0){
            sgarray_cpp2c(&sga, sga_ptr);
        }
    }
    return ret;
}

ssize_t zeus_wait_any(zeus_qtoken *qts, size_t num_qts, zeus_sgarray *sga_list) {
    return -1;
}

ssize_t zeus_wait_all(zeus_qtoken *qts, size_t num_qts, zeus_sgarray *sga_list) {
    return -1;
}

// identical to a push, followed by a wait on the returned zeus_qtoken
ssize_t zeus_blocking_push(int qd, zeus_sgarray *sga_ptr) {
    return -1;
}

// identical to a pop, followed by a wait on the returned zeus_qtoken
ssize_t zeus_blocking_pop(int qd, zeus_sgarray *sga_ptr) {
    return -1;
}

int zeus_qd2fd(int qd){
    return Zeus::qd2fd(qd);
}

int zeus_init(int argc, char* argv[]) {
	return Zeus::init(argc, argv);
}


