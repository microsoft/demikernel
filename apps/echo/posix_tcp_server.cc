#include <netinet/in.h>
#include <unistd.h>
#include <arpa/inet.h>

#include <boost/optional.hpp>
#include <boost/program_options/options_description.hpp>
#include <boost/program_options/parsers.hpp>
#include <boost/program_options/variables_map.hpp>
#include <cassert>
#include <cstring>
#include <dmtr/annot.h>
#include <dmtr/wait.h>
#include <dmtr/libos.h>
#include <iostream>
#include <libos/common/mem.h>

#include <yaml-cpp/yaml.h>
#include <stdio.h>
#include <sys/epoll.h>
#include <vector>
#include <netinet/tcp.h>
#include <arpa/inet.h>
#include <netinet/in.h>
#include <fcntl.h>
#include <unistd.h>
#include <sys/types.h>
#include <sys/socket.h>
#include <netdb.h>
#include <signal.h>

#include "common.hh"

int lqd = 0;
dmtr_timer_t *pop_timer = NULL;
dmtr_timer_t *push_timer = NULL;

namespace po = boost::program_options;
int lfd = 0, epoll_fd;

void sig_handler(int signo)
{
    dmtr_dump_timer(stderr, pop_timer);
    dmtr_dump_timer(stderr, push_timer);
    close(lfd);
    close(epoll_fd);
    exit(0);
}


int process_read(int fd, char *buf)
{
    int bytes_read = 0, ret;
    DMTR_OK(dmtr_start_timer(pop_timer));
    while (bytes_read < (int)packet_size) {
        ret = read(fd,
                   (void *)&(buf[bytes_read]),
                   packet_size - bytes_read);
        if (ret < 0) {
            close(fd);
            return ret;
        }
        bytes_read += ret;
    }
    DMTR_OK(dmtr_stop_timer(pop_timer));
    return bytes_read;
}

int process_write(int fd, char *buf)
{
    int bytes_written = 0, ret;
    DMTR_OK(dmtr_start_timer(push_timer));
    while (bytes_written < (int)packet_size) {
        ret = write(fd,
                    (void *)&(buf[bytes_written]),
                    packet_size - bytes_written);
        if (ret < 0) {
            close(fd);
            return ret;
        }
        bytes_written += ret;
    }                         
    DMTR_OK(dmtr_stop_timer(push_timer));
    return bytes_written;
}
 

int main(int argc, char *argv[])
{
    parse_args(argc, argv, true);

    struct sockaddr_in saddr = {};
    saddr.sin_family = AF_INET;
    if (boost::none == server_ip_addr) {
        std::cerr << "Listening on `*:" << port << "`..." << std::endl;
        saddr.sin_addr.s_addr = INADDR_ANY;
    } else {
        const char *s = boost::get(server_ip_addr).c_str();
        std::cerr << "Listening on `" << s << ":" << port << "`..." << std::endl;
        if (inet_pton(AF_INET, s, &saddr.sin_addr) != 1) {
            std::cerr << "Unable to parse IP address." << std::endl;
            return -1;
        }
    }
    saddr.sin_port = htons(port);

    DMTR_OK(dmtr_new_timer(&pop_timer, "server pop"));
    DMTR_OK(dmtr_new_timer(&push_timer, "server push"));

    lfd = socket(AF_INET, SOCK_STREAM, 0);
    std::cout << "listen qd: " << lfd << std::endl;

    // Put it in non-blocking mode
    DMTR_OK(fcntl(lfd, F_SETFL, O_NONBLOCK, 1));

    // Set TCP_NODELAY
    int n = 1;
    if (setsockopt(lfd, IPPROTO_TCP,
                   TCP_NODELAY, (char *)&n, sizeof(n)) < 0) {
        exit(-1);
    }
    
    DMTR_OK(bind(lfd, reinterpret_cast<struct sockaddr *>(&saddr), sizeof(saddr)));

    std::cout << "listening for connections\n";
    listen(lfd, 3);

    if (signal(SIGINT, sig_handler) == SIG_ERR)
        std::cout << "\ncan't catch SIGINT\n";
    if (signal(SIGPIPE, sig_handler) == SIG_ERR)
        std::cout << "\ncan't catch SIGPIPE\n";

    epoll_fd = epoll_create1(0);
    struct epoll_event event, events[10];
    event.events = EPOLLIN;
    event.data.fd = lfd;
    DMTR_OK(epoll_ctl(epoll_fd, EPOLL_CTL_ADD, lfd, &event));
    while (1) {
        int event_count = epoll_wait(epoll_fd, events, 10, -1);
        for (int i = 0; i < event_count; i++) {
            //std::cout << "Found something!" << std::endl;
            if (events[i].data.fd == lfd) {
                // run accept
                std::cout << "Found new connection" << std::endl;
                int newfd = accept(lfd, NULL, NULL);

                // Put it in non-blocking mode
                DMTR_OK(fcntl(newfd, F_SETFL, O_NONBLOCK, 1));
                
                // Set TCP_NODELAY
                int n = 1;
                if (setsockopt(newfd, IPPROTO_TCP,
                               TCP_NODELAY, (char *)&n, sizeof(n)) < 0) {
                    exit(-1);
                }

                event.events = EPOLLIN;
                event.data.fd = newfd;
                DMTR_OK(epoll_ctl(epoll_fd, EPOLL_CTL_ADD, newfd, &event));
            } else {
                char *buf = (char *)malloc(packet_size);
                if (process_read(events[i].data.fd, buf) < 0) {
                    free(buf);
                    continue;
                }
                if (process_write(events[i].data.fd, buf) < 0) {
                    free(buf);
                    continue;
                }
                free(buf);                
            }
        }
    }
}


