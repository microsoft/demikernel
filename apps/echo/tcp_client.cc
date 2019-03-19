#include <dmtr/annot.h>
#include <dmtr/libos.h>
#include <libos/common/mem.h>
#include <dmtr/wait.h>

#include <arpa/inet.h>
#include <boost/optional.hpp>
#include <boost/program_options/options_description.hpp>
#include <boost/program_options/parsers.hpp>
#include <boost/program_options/variables_map.hpp>
#include <cstring>
#include <iostream>
#include <netinet/in.h>
#include <unistd.h>
#include <yaml-cpp/yaml.h>

#define ITERATION_COUNT 10000
#define BUFFER_SIZE 10
#define FILL_CHAR 'a'

namespace po = boost::program_options;

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
    std::string server_ip_addr = "127.0.0.1";
    uint16_t port = 12345;
    YAML::Node node = config["client"]["connect_to"]["host"];
    if (YAML::NodeType::Scalar == node.Type()) {
        server_ip_addr = node.as<std::string>();
    }
    node = config["client"]["connect_to"]["port"];
    if (YAML::NodeType::Scalar == node.Type()) {
        port = node.as<uint16_t>();
    }

    struct sockaddr_in saddr = {};
    saddr.sin_family = AF_INET;
    saddr.sin_port = htons(port);
    if (inet_pton(AF_INET, server_ip_addr.c_str(), &saddr.sin_addr) != 1) {
        std::cerr << "Unable to parse IP address." << std::endl;
        return -1;
    }

    DMTR_OK(dmtr_init(argc, argv));

    dmtr_timer_t *pop_timer = NULL;
    DMTR_OK(dmtr_newtimer(&pop_timer, "pop"));
    dmtr_timer_t *push_timer = NULL;
    DMTR_OK(dmtr_newtimer(&push_timer, "push"));

    int qd = 0;
    DMTR_OK(dmtr_socket(&qd, AF_INET, SOCK_STREAM, 0));
    printf("client qd:\t%d\n", qd);

    std::cerr << "Attempting to connect to `" << server_ip_addr << ":" << port << "`..." << std::endl;
    DMTR_OK(dmtr_connect(qd, reinterpret_cast<struct sockaddr *>(&saddr), sizeof(saddr)));
    std::cerr << "Connected." << std::endl;

    dmtr_sgarray_t sga = {};
    void *p = NULL;
    DMTR_OK(dmtr_malloc(&p, BUFFER_SIZE));
    char *s = reinterpret_cast<char *>(p);
    memset(s, FILL_CHAR, BUFFER_SIZE);
    s[BUFFER_SIZE - 1] = '\0';
    sga.sga_numsegs = 1;
    sga.sga_segs[0].sgaseg_len = BUFFER_SIZE;
    sga.sga_segs[0].sgaseg_buf = p;

    for (size_t i = 0; i < ITERATION_COUNT; i++) {
        dmtr_qtoken_t qt;
        DMTR_OK(dmtr_starttimer(pop_timer));
        DMTR_OK(dmtr_push(&qt, qd, &sga));
        DMTR_OK(dmtr_wait(NULL, qt));
        //DMTR_OK(dmtr_stoptimer(pop_timer));
        //fprintf(stderr, "send complete.\n");

        dmtr_qresult_t qr = {};
        //DMTR_OK(dmtr_starttimer(push_timer));
        DMTR_OK(dmtr_pop(&qt, qd));
        DMTR_OK(dmtr_wait(&qr, qt));
        DMTR_OK(dmtr_stoptimer(pop_timer));
        assert(DMTR_OPC_POP == qr.qr_opcode);
        assert(qr.qr_value.sga.sga_numsegs == 1);
        assert(reinterpret_cast<uint8_t *>(qr.qr_value.sga.sga_segs[0].sgaseg_buf)[0] == FILL_CHAR);

        /*fprintf(stderr, "[%lu] client: rcvd\t%s\tbuf size:\t%d\n", i, reinterpret_cast<char *>(qr.qr_value.sga.sga_segs[0].sgaseg_buf), qr.qr_value.sga.sga_segs[0].sgaseg_len);*/
        free(qr.qr_value.sga.sga_buf);
    }

    DMTR_OK(dmtr_dumptimer(stderr, pop_timer));
    //    DMTR_OK(dmtr_dumptimer(stderr, push_timer));
    DMTR_OK(dmtr_close(qd));

    return 0;
}
