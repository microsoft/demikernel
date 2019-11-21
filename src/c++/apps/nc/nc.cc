#include <string>
#include <iostream>
#include <fstream>
#include <thread>
#include <csignal>

#include <arpa/inet.h>

#include <boost/program_options.hpp>

#include <dmtr/fail.h>
#include <dmtr/libos.h>
#include <dmtr/wait.h>

namespace boost_opts = boost::program_options;

struct ArgumentOpts {
    std::string ip;
    uint16_t port;
};

int parse_args(int argc, char **argv, ArgumentOpts &options) {
    boost_opts::options_description opts{"KV Server options"};
    opts.add_options()
                    ("help", "produce help message")
                    ("ip",
                        boost_opts::value<std::string>(&options.ip)->default_value("127.0.0.1"),
                        "Server IP")
                    ("port",
                        boost_opts::value<uint16_t>(&options.port)->default_value(12345),
                        "Server port");

    boost_opts::variables_map vm;
    try {
        boost_opts::parsed_options parsed =
            boost_opts::command_line_parser(argc, argv).options(opts).run();
        boost_opts::store(parsed, vm);
        if (vm.count("help")) {
            std::cout << opts << std::endl;
            exit(0);
        }
        boost_opts::notify(vm);
    } catch (const boost_opts::error &e) {
        std::cerr << e.what() << std::endl;
        std::cerr << opts << std::endl;
        return 1;
    }
    return 0;
}

int main(int argc, char **argv) {

    ArgumentOpts opts;
    if (parse_args(argc, argv, opts)) {
        return 1;
    }

    printf("Launching kv store on %s:%u\n", opts.ip.c_str(), opts.port);

    struct sockaddr_in addr = {};
    addr.sin_family = AF_INET;
    if (inet_pton(AF_INET, opts.ip.c_str(), &addr.sin_addr) != 1) {
        printf("Could not convert %s to ip\n", opts.ip.c_str());
        return -1;
    }
    addr.sin_port = htons(opts.port);

    DMTR_OK(dmtr_init(argc, argv));
    int qd;
    DMTR_OK(dmtr_socket(&qd, AF_INET, SOCK_STREAM, 0));

    DMTR_OK(dmtr_connect(qd, reinterpret_cast<struct sockaddr*>(&addr), sizeof(addr)));

    for (std::string line; std::getline(std::cin, line);) {
        dmtr_qtoken_t token;
        dmtr_sgarray_t sga;
        sga.sga_numsegs = 1;
        sga.sga_segs[0].sgaseg_len = line.size();
        sga.sga_segs[0].sgaseg_buf = (void*)line.c_str();
        DMTR_OK(dmtr_push(&token, qd, &sga));
        int status;
        while ((status = dmtr_wait(NULL, token)) == EAGAIN) {}

        DMTR_OK(dmtr_pop(&token, qd));

        dmtr_qresult_t wait_out;
        while ((status = dmtr_wait(&wait_out, token)) == EAGAIN) {
            std::this_thread::sleep_for(std::chrono::milliseconds(100));
        }
        printf("Recvd: %.*s\n", wait_out.qr_value.sga.sga_segs[0].sgaseg_len,
                                (char*)wait_out.qr_value.sga.sga_segs[0].sgaseg_buf);
    }
    return 0;
}
