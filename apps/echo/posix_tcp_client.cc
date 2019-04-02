#include <dmtr/annot.h>
#include <dmtr/libos.h>
#include <libos/common/mem.h>
#include <dmtr/wait.h>

#include <netinet/tcp.h>
#include <arpa/inet.h>
#include <netinet/in.h>
#include <fcntl.h>
#include <unistd.h>
#include <sys/types.h>
#include <sys/socket.h>
#include <netdb.h>
#include <signal.h>

#include <boost/optional.hpp>
#include <boost/program_options/options_description.hpp>
#include <boost/program_options/parsers.hpp>
#include <boost/program_options/variables_map.hpp>
#include <cstring>
#include <iostream>
#include <yaml-cpp/yaml.h>
#include "common.hh"

#define FILL_CHAR 'a'

namespace po = boost::program_options;

int main(int argc, char *argv[])
{
    parse_args(argc, argv, false);

    struct sockaddr_in saddr = {};
    saddr.sin_family = AF_INET;
    saddr.sin_port = htons(port);
    if (inet_pton(AF_INET, server_ip_addr->c_str(), &saddr.sin_addr) != 1) {
        std::cerr << "Unable to parse IP address." << std::endl;
        return -1;
    }


    dmtr_timer_t *timer = NULL;
    DMTR_OK(dmtr_new_timer(&timer, "end-to-end"));
    dmtr_timer_t *pop_timer = NULL;
    DMTR_OK(dmtr_new_timer(&pop_timer, "pop"));
    dmtr_timer_t *push_timer = NULL;
    DMTR_OK(dmtr_new_timer(&push_timer, "push"));

    int fd = socket(AF_INET, SOCK_STREAM, 0);
    printf("client fd:\t%d\n", fd);

    // Set TCP_NODELAY
    int n = 1;
    if (setsockopt(fd, IPPROTO_TCP,
                   TCP_NODELAY, (char *)&n, sizeof(n)) < 0) {
        exit(-1);
    }

    std::cerr << "Attempting to connect to `" << boost::get(server_ip_addr) << ":" << port << "`..." << std::endl;
    DMTR_OK(connect(fd, reinterpret_cast<struct sockaddr *>(&saddr), sizeof(saddr)));
    std::cerr << "Connected." << std::endl;

    char buf[packet_size];
    memset(&buf, FILL_CHAR, packet_size);
    buf[packet_size - 1] = '\0';

    for (size_t i = 0; i < iterations; i++) {
        DMTR_OK(dmtr_start_timer(timer));
        DMTR_OK(dmtr_start_timer(push_timer));
        int bytes_written = 0, ret;
        while (bytes_written < (int)packet_size) {
            ret = write(fd,
                  (void *)&(buf[bytes_written]),
                  packet_size-bytes_written);
            if (ret < 0) {
              exit(-1);
            }
            bytes_written += ret;
        }
        DMTR_OK(dmtr_stop_timer(push_timer));
        DMTR_OK(dmtr_start_timer(pop_timer));
        int bytes_read = 0;
        while(bytes_read < (int)packet_size) {
            ret += read(fd, (void *)&buf, packet_size);
            if (ret < 0) exit(-1);
            bytes_read += ret;
        }
        DMTR_OK(dmtr_stop_timer(pop_timer));
        DMTR_OK(dmtr_stop_timer(timer));
    }
    close(fd);
    DMTR_OK(dmtr_dump_timer(stderr, timer));
    DMTR_OK(dmtr_dump_timer(stderr, pop_timer));
    DMTR_OK(dmtr_dump_timer(stderr, push_timer));
    return 0;
}
