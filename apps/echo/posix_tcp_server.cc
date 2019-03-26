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
#include <dmtr/libos.h>
#include <dmtr/wait.h>
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


#define ITERATION_COUNT 10000
#define MAX_EVENTS 10
#define PACKET_SIZE 1024

namespace po = boost::program_options;
int lfd = 0, epoll_fd;
dmtr_timer_t *pop_timer = NULL;
dmtr_timer_t *push_timer = NULL;


void sig_handler(int signo)
{
  dmtr_dump_timer(stderr, pop_timer);
  dmtr_dump_timer(stderr, push_timer);
  close(lfd);
  close(epoll_fd);
  exit(0);
}

int main(int argc, char *argv[])
{
    std::string config_path;
    po::options_description desc("Allowed options");
    desc.add_options()
        ("help", "display usage information")
        ("config-path,c", po::value<std::string>(&config_path)->default_value("./config.yaml"), "specify configuration file");

    po::variables_map vm;
    po::store(po::parse_command_line(argc, argv, desc), vm);
    po::notify(vm);

    if (vm.count("help")) {
        std::cout << desc << std::endl;
        return 0;
    }

    if (access(config_path.c_str(), R_OK) == -1) {
        std::cerr << "Unable to find config file at `" << config_path << "`." << std::endl;
        return -1;
    }
    
    YAML::Node config = YAML::LoadFile(config_path);
    boost::optional<std::string> server_ip_addr;
    uint16_t port = 12345;
    YAML::Node node = config["server"]["bind"]["host"];
    if (YAML::NodeType::Scalar == node.Type()) {
        server_ip_addr = node.as<std::string>();
    }
    node = config["server"]["bind"]["port"];
    if (YAML::NodeType::Scalar == node.Type()) {
        port = node.as<uint16_t>();
    }

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

    DMTR_OK(dmtr_new_timer(&pop_timer, "pop"));
    DMTR_OK(dmtr_new_timer(&push_timer, "push"));

    lfd = socket(AF_INET, SOCK_STREAM, 0);
    std::cout << "listen qd: " << lfd;

    // Put it in non-blocking mode
    DMTR_OK(fcntl(lfd, F_SETFL, O_NONBLOCK, 1));

    // Set TCP_NODELAY
    int n = 1;
    if (setsockopt(lfd, IPPROTO_TCP,
                   TCP_NODELAY, (char *)&n, sizeof(n)) < 0) {
        exit(1);
    }
    
    DMTR_OK(bind(lfd, reinterpret_cast<struct sockaddr *>(&saddr), sizeof(saddr)));

    std::cout << "listening for connections\n";
    listen(lfd, 3);

    if (signal(SIGINT, sig_handler) == SIG_ERR)
        std::cout << "\ncan't catch SIGINT\n";

    epoll_fd = epoll_create1(0);
    struct epoll_event event, events[10];
    event.events = EPOLLIN;
    event.data.fd = lfd;
    DMTR_OK(epoll_ctl(epoll_fd, EPOLL_CTL_ADD, 0, &event));
    char buf[PACKET_SIZE];
    while (1) {
        int event_count = epoll_wait(epoll_fd, events, 10, -1);
        for (int i = 0; i < event_count; i++) {
            if (events[i].data.fd == lfd) {
                // run accept
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
                DMTR_OK(epoll_ctl(epoll_fd, EPOLL_CTL_ADD, 0, &event));
            } else {
                int bytes_written = 0;
                int bytes_read = read(events[i].data.fd, (void *)&buf, PACKET_SIZE);
                if (bytes_read < PACKET_SIZE)
                    continue;
                while (bytes_written < PACKET_SIZE) {
                    bytes_written += write(events[i].data.fd, (void *)&buf, PACKET_SIZE);
                }
            }
        }
    }
}


