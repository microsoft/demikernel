#include <dmtr/annot.h>
#include <dmtr/libos.h>
#include <libos/common/mem.h>
#include <dmtr/wait.h>

#include <arpa/inet.h>
#include <boost/optional.hpp>
#include <boost/program_options/options_description.hpp>
#include <boost/program_options/parsers.hpp>
#include <boost/program_options/variables_map.hpp>
#include <cassert>
#include <cstring>
#include <iostream>
#include <netinet/in.h>
#include <unistd.h>
#include <yaml-cpp/yaml.h>

#define ITERATION_COUNT 10000

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
    saddr.sin_port = port;

    DMTR_OK(dmtr_init(argc, argv));

    dmtr_timer_t *pop_timer = NULL;
    DMTR_OK(dmtr_newtimer(&pop_timer, "pop"));
    dmtr_timer_t *push_timer = NULL;
    DMTR_OK(dmtr_newtimer(&push_timer, "push"));

    int lqd = 0;
    DMTR_OK(dmtr_socket(&lqd, AF_INET, SOCK_STREAM, 0));
    printf("listen qd:\t%d\n", lqd);

    DMTR_OK(dmtr_bind(lqd, reinterpret_cast<struct sockaddr *>(&saddr), sizeof(saddr)));

    printf("listening for connections\n");
    DMTR_OK(dmtr_listen(lqd, 3));

    dmtr_qtoken_t qt = 0;
    dmtr_qresult_t qr = {};
    DMTR_OK(dmtr_accept(&qt, lqd));
    DMTR_OK(dmtr_wait(&qr, qt));
    DMTR_OK(dmtr_drop(qt));
    DMTR_TRUE(EPERM, DMTR_OPC_ACCEPT == qr.qr_opcode);
    DMTR_TRUE(EPERM, DMTR_TID_QD == qr.qr_tid);
    int qd = qr.qr_value.qd;
    std::cerr << "Connection accepted." << std::endl;

    // process ITERATION_COUNT packets from client
    for (size_t i = 0; i < ITERATION_COUNT; i++) {
        DMTR_OK(dmtr_starttimer(pop_timer));
        DMTR_OK(dmtr_pop(&qt, qd));
        DMTR_OK(dmtr_wait(&qr, qt));
        DMTR_OK(dmtr_stoptimer(pop_timer));
        DMTR_OK(dmtr_drop(qt));
        DMTR_TRUE(EPERM, DMTR_OPC_POP == qr.qr_opcode);
        DMTR_TRUE(EPERM, DMTR_TID_SGA == qr.qr_tid);
        DMTR_TRUE(EPERM, qr.qr_value.sga.sga_numsegs == 1);

        /*fprintf(stderr, "[%lu] server: rcvd\t%s\tbuf size:\t%d\n", i, reinterpret_cast<char *>(qr.qr_value.sga.sga_segs[0].sgaseg_buf), qr.qr_value.sga.sga_segs[0].sgaseg_len);*/
        DMTR_OK(dmtr_starttimer(push_timer));
        DMTR_OK(dmtr_push(&qt, qd, &qr.qr_value.sga));
        DMTR_OK(dmtr_wait(NULL, qt));
        DMTR_OK(dmtr_stoptimer(push_timer));
        DMTR_OK(dmtr_drop(qt));

        //fprintf(stderr, "send complete.\n");
        free(qr.qr_value.sga.sga_buf);
    }

    DMTR_OK(dmtr_dumptimer(stderr, pop_timer));
    DMTR_OK(dmtr_dumptimer(stderr, push_timer));
    DMTR_OK(dmtr_close(qd));
    DMTR_OK(dmtr_close(lqd));

    return 0;
}
