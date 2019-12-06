#include <iostream>

#include <boost/optional.hpp>
#include <boost/program_options.hpp>

#include <dmtr/libos/persephone.hh>

namespace bpo = boost::program_options;

int main(int argc, char *argv[]) {
    std::string app_cfg;
    bpo::options_description desc{"Psp echo server options"};
    desc.add_options()
    ("help", "produce help message")
    ("app-cfg,c", bpo::value<std::string>(&app_cfg)->required(), "Specify application configuration file");

    bpo::variables_map vm;
    try {
        bpo::parsed_options parsed =
            bpo::command_line_parser(argc, argv).options(desc).run();
        bpo::store(parsed, vm);
        if (vm.count("help")) {
            std::cout << desc << std::endl;
            return 1;
        }
        bpo::notify(vm);
    } catch (const bpo::error &e) {
        std::cerr << e.what() << std::endl;
        std::cerr << desc << std::endl;
        return 1;
    }

    Psp psp(app_cfg);

    return 0;
}
