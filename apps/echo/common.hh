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
        ("ip", value<std::string>(&ip)->default_value("127.0.0.1"), "server ip address")
        ("port", value<uint16_t>(&port)->default_value(12345), "server port")
        ("size,s", value<uint32_t>(&packet_size)->default_value(64), "packet payload size")
        ("iterations,i", value<uint32_t>(&iterations)->default_value(10), "test iterations");

    variables_map vm;
    store(parse_command_line(argc, argv, desc), vm);

    // print help
    if (vm.count("help")) {
        std::cout << desc << "\n";
        exit(0);
    }
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
