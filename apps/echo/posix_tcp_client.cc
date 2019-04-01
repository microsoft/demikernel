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

#define ITERATION_COUNT 10000
#define BUFFER_SIZE 1024
#define FILL_CHAR 'a'

namespace po = boost::program_options;

int main(int argc, char *argv[])
{
    parse_args(argc, argv, false);

    struct sockaddr_in saddr = {};
    saddr.sin_family = AF_INET;
    std::cerr << "Listening on `*:" << port << "`..." << std::endl;
    saddr.sin_addr.s_addr = INADDR_ANY;
    saddr.sin_port = htons(port);
    dmtr_timer_t *timer = NULL;
    DMTR_OK(dmtr_new_timer(&timer, "timer"));

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

    char buf[BUFFER_SIZE];
    memset(&buf, FILL_CHAR, BUFFER_SIZE);
    buf[BUFFER_SIZE - 1] = '\0';
 
    for (size_t i = 0; i < ITERATION_COUNT; i++) {
      DMTR_OK(dmtr_start_timer(timer));
	int bytes_written = 0, ret;
	while (bytes_written < BUFFER_SIZE) {
	  ret = write(fd,
		      (void *)&(buf[bytes_written]),
		      BUFFER_SIZE-bytes_written);
	  if (ret < 0) {
	    exit(-1);
	  }
	  bytes_written += ret;
	}
	int bytes_read = 0;
        while(bytes_read < BUFFER_SIZE) {
            ret += read(fd, (void *)&buf, BUFFER_SIZE);
	    if (ret < 0) exit(-1);
	    bytes_read += ret;
        }
	DMTR_OK(dmtr_stop_timer(timer));
    }
    close(fd);
    DMTR_OK(dmtr_dump_timer(stderr, timer));
    return 0;
}
