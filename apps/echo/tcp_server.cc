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
#include <netinet/in.h>
#include <unistd.h>
#include <yaml-cpp/yaml.h>
#include <stdio.h>
#include <signal.h>
#include <vector>

#define ITERATION_COUNT 10000

namespace po = boost::program_options;
int lqd = 0;
dmtr_timer_t *pop_timer = NULL;
dmtr_timer_t *push_timer = NULL;

void sig_handler(int signo)
{
    if (signo == SIGUSR1 || signo == SIGKILL || signo == SIGSTOP) {
        dmtr_dump_timer(stderr, pop_timer);
        dmtr_dump_timer(stderr, push_timer);
        dmtr_close(lqd);
    }
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

    if (signal(SIGUSR1, sig_handler) == SIG_ERR)
        std::cout << "\ncan't catch SIGUSR1\n";
    if (signal(SIGKILL, sig_handler) == SIG_ERR)
        std::cout << "\ncan't catch SIGKILL\n";
    if (signal(SIGSTOP, sig_handler) == SIG_ERR)
        std::cout << "\ncan't catch SIGSTOP\n";
    
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

    DMTR_OK(dmtr_init(argc, argv));
    DMTR_OK(dmtr_new_timer(&pop_timer, "pop"));
    DMTR_OK(dmtr_new_timer(&push_timer, "push"));

    std::vector<dmtr_qtoken_t> tokens;
    dmtr_qtoken_t token;
    DMTR_OK(dmtr_socket(&lqd, AF_INET, SOCK_STREAM, 0));
    std::cout << "listen qd: " << lqd;

    DMTR_OK(dmtr_bind(lqd, reinterpret_cast<struct sockaddr *>(&saddr), sizeof(saddr)));

    std::cout << "listening for connections\n";
    DMTR_OK(dmtr_listen(lqd, 3));
    DMTR_OK(dmtr_accept(&token, lqd));
    tokens.push_back(token);
    while (1) {
        dmtr_qresult wait_out;
        int idx;
        int status = dmtr_wait_any(&wait_out, &idx, tokens.data(), tokens.size());

        // if we got an EOK back from wait
        if (status == 0) {
            std::cout << "Found something: qd=" << wait_out.qr_qd;

            if (wait_out.qr_qd == lqd) {
                // check accept on servers
                DMTR_OK(dmtr_start_timer(pop_timer));
                DMTR_OK(dmtr_pop(&token, wait_out.qr_value.ares.qd));
                DMTR_OK(dmtr_stop_timer(pop_timer));
                tokens.push_back(token);
                DMTR_OK(dmtr_accept(&token, lqd));
                tokens[idx] = token;
            } else {
                assert(DMTR_OPC_POP == wait_out.qr_opcode);
                assert(wait_out.qr_value.sga.sga_numsegs == 1);
                //fprintf(stderr, "[%lu] server: rcvd\t%s\tbuf size:\t%d\n", i, reinterpret_cast<char *>(qr.qr_value.sga.sga_segs[0].sgaseg_buf), qr.qr_value.sga.sga_segs[0].sgaseg_len);
                DMTR_OK(dmtr_start_timer(push_timer));
                DMTR_OK(dmtr_push(&token, wait_out.qr_qd, &wait_out.qr_value.sga));
                DMTR_OK(dmtr_wait(NULL, token));
                DMTR_OK(dmtr_stop_timer(push_timer));
                DMTR_OK(dmtr_start_timer(pop_timer));
                DMTR_OK(dmtr_pop(&token, wait_out.qr_qd));
                DMTR_OK(dmtr_stop_timer(pop_timer));
                tokens[idx] = token;
                //fprintf(stderr, "send complete.\n");
                free(wait_out.qr_value.sga.sga_buf);
            }
        } else {
            assert(status == ECONNRESET || status == ECONNABORTED);
            dmtr_close(wait_out.qr_qd);
            tokens.erase(tokens.begin()+idx);
        }
    }
}


