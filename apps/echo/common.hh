#ifndef ECHO_COMMON_H_
#define ECHO_COMMON_H_

#include <string.h>
#include <dmtr/annot.h>
#include <dmtr/mem.h>
#include <iostream>
#include <boost/program_options.hpp>

uint16_t port = 12345;
std::string ip = "127.0.0.1";
uint32_t packet_size = 64;
uint32_t iterations = 10;
int dmtr_argc = 0;
char **dmtr_argv = NULL;

using namespace boost::program_options;

void parse_args(int argc, char **argv)
{
    options_description desc{"echo experiment options"};
    desc.add_options()
        ("help", "produce help message")
        ("ip", "server ip address")
        ("port", "server port")
        ("size, s", "packet payload size")
        ("iterations, i", "test iterations");

    variables_map vm;
    store(parse_command_line(argc, argv, desc), vm);

    // pick up arguments
    if (vm.count("help"))
        std::cout << desc << "\n";
    if (vm.count("ip"))
        ip = vm["ip"].as<std::string>();
    if (vm.count("port"))
        port = vm["port"].as<uint16_t>();
    if (vm.count("size"))
        packet_size = vm["size"].as<uint32_t>();
    if (vm.count("iterations"))
        iterations = vm["iterations"].as<uint32_t>();
    
};

void* generate_packet()
{
    void *p = NULL;
    dmtr_malloc(&p, packet_size);
    char *s = reinterpret_cast<char *>(p);
    memset(s, 'a', packet_size);
    s[packet_size - 1] = '\0';
    return p;
};

#endif
