// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#ifndef ECHO_COMMON_H_
#define ECHO_COMMON_H_

#include <boost/optional.hpp>
#include <boost/program_options.hpp>
#include <dmtr/annot.h>
#include <iostream>
#include <dmtr/libos/mem.h>
#include <string.h>
#include <yaml-cpp/yaml.h>

uint16_t port = 12345;
boost::optional<std::string> server_ip_addr;
uint32_t packet_size = 64;
uint32_t iterations = 10;
const char FILL_CHAR = 'a';
boost::optional<std::string> file;

using namespace boost::program_options;

void parse_args(int argc, char **argv, bool server)
{
    std::string config_path;
    options_description desc{"echo experiment options"};
    desc.add_options()
        ("help", "produce help message")
        ("ip", value<std::string>(), "server ip address")
        ("port", value<uint16_t>(&port)->default_value(12345), "server port")
        ("size,s", value<uint32_t>(&packet_size)->default_value(64), "packet payload size")
        ("iterations,i", value<uint32_t>(&iterations)->default_value(10), "test iterations")
        ("config-path,c", value<std::string>(&config_path)->default_value("./config.yaml"), "specify configuration file")
        ("file", value<std::string>(), "log file");

    variables_map vm;
    store(command_line_parser(argc, argv).options(desc).allow_unregistered().run(), vm);
    notify(vm);

    // print help
    if (vm.count("help")) {
        std::cout << desc << std::endl;
        exit(0);
    }

    if (!server) {
        server_ip_addr = "127.0.0.1";
    }

    if (access(config_path.c_str(), R_OK) == -1) {
        std::cerr << "Unable to find config file at `" << config_path << "`." << std::endl;
    } else {
        YAML::Node config = YAML::LoadFile(config_path);
        if (server) {
            YAML::Node node = config["server"]["bind"]["host"];
            if (YAML::NodeType::Scalar == node.Type()) {
                server_ip_addr = node.as<std::string>();
            }

            node = config["server"]["bind"]["port"];
            if (YAML::NodeType::Scalar == node.Type()) {
                port = node.as<uint16_t>();
            }
        } else {
            YAML::Node node = config["client"]["connect_to"]["host"];
            if (YAML::NodeType::Scalar == node.Type()) {
                server_ip_addr = node.as<std::string>();
            }

            node = config["client"]["connect_to"]["port"];
            if (YAML::NodeType::Scalar == node.Type()) {
                port = node.as<uint16_t>();
            }
        }
    }

    if (vm.count("ip")) {
        server_ip_addr = vm["ip"].as<std::string>();
        //std::cout << "Setting server IP to: " << ip << std::endl;
    }

    if (vm.count("port")) {
        port = vm["port"].as<uint16_t>();
        //std::cout << "Setting server port to: " << port << std::endl;
    }

    if (vm.count("iterations")) {
        iterations = vm["iterations"].as<uint32_t>();
        //std::cout << "Setting iterations to: " << iterations << std::endl;
    }

    if (vm.count("size")) {
        packet_size = vm["size"].as<uint32_t>();
        //std::cout << "Setting packet size to: " << packet_size << " bytes." << std::endl;
    }

    if (vm.count("file")) {
        file = vm["file"].as<std::string>();
    }
};

void* generate_packet()
{
    void *p = NULL;
    dmtr_malloc(&p, packet_size);
    char *s = reinterpret_cast<char *>(p);
    memset(s, FILL_CHAR, packet_size);
    s[packet_size - 1] = '\0';
    return p;
};

#endif
