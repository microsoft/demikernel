#include <iostream>
#include <string.h>
#include <netinet/in.h>
#include <assert.h>
#include <arpa/inet.h>
#include <boost/program_options.hpp>

using namespace boost::program_options;

uint16_t port = 12345;
uint32_t ip = INADDR_ANY;
uint32_t packet_size = 64;
uint32_t iterations = 10;
int dmtr_argc = 0;
char **dmtr_argv = NULL;

 parse_args(int argv, char **argv)
{

    options_description("echo experiment options");
    desc.add_options()
        ("help", "produce help message")
        ("ip", "server ip address")
        ("port", "server port")
        ("size, s", "packet payload size")
        ("iterations, i", "test iterations");
    
}
