#include <iostream>

#include <arpa/inet.h>
#include <netinet/in.h>
#include <yaml-cpp/yaml.h>

#include <dmtr/libos.h>
#include <dmtr/libos/persephone.hh>
#include <dmtr/annot.h>

/********** CONTROL PLANE ******************/
//TODO:
// - don't create network context for SU that specify a non existing device
Psp::Psp(std::string &app_cfg) {
    /* Let network libOS init its specific EAL */
    dmtr_net_init(app_cfg.c_str());

    /* Allocate a mempool for the network */
    net_ctx.net_mempool = NULL;
    dmtr_net_mempool_init(&net_ctx.net_mempool, 0);

    /* Parse the configuration */
    std::unordered_map<uint16_t, uint32_t> devices_to_sus;
    try {
        YAML::Node config = YAML::LoadFile(app_cfg);
        YAML::Node sus = config["service_units"];
        for (size_t i = 0; i < sus.size(); ++i) {
            std::shared_ptr<PspServiceUnit> service_unit = std::make_shared<PspServiceUnit>(i);
            for (auto su = sus[i].begin(); su != sus[i].end(); ++su) {
                auto key = su->first.as<std::string>();
                auto value = su->second;
                if (key == "io") {
                    // Iterate through all the queue types for this SU
                    for (size_t j = 0; j < value.size(); ++j) {
                        auto ioq = value[j];
                        //std::cout << ioq << std::endl;
                        if (ioq["type"].as<std::string>() == "NETWORK") {
                            if (service_unit->net_context_init_flag) {
                                std::cerr << "Service unit's net context already initialized" << std::endl;
                                continue;
                            }
                            auto dev_id = ioq["device_id"].as<uint16_t>();
                            auto it = devices_to_sus.find(dev_id);
                            if (it == devices_to_sus.end()) {
                                devices_to_sus.insert(
                                    std::pair<uint16_t, uint32_t>(dev_id, 1)
                                );
                            } else {
                                devices_to_sus[dev_id]++;
                            }
                            /* Retrieve the default IP for the service unit */
                            struct in_addr ip;
                            service_unit->ip = ioq["ip"].as<std::string>();
                            inet_aton(service_unit->ip.c_str(), &ip);
                            /* Set a new network context for the service unit */
                            int rtn = dmtr_init_net_context(
                                &service_unit->io_ctx.net_context,
                                net_ctx.net_mempool,
                                dev_id, devices_to_sus[dev_id]-1,
                                ip
                            );
                            assert(service_unit->io_ctx.net_context != NULL);
                            if (rtn != 0) {
                                std::cerr << "Error setting up service unit " << i;
                                std::cerr << " net context" << std::endl;
                                exit(1);
                            }
                            /* Retrieve the port */
                            std::cout << ioq["port"] << std::endl;
                            service_unit->port = ioq["port"].as<uint16_t>();
                            service_unit->net_context_init_flag = true;
                        }
                    }
                }
            }
            service_units[i] = service_unit;
        }
        if (config["log_dir"].IsDefined()) {
            log_dir = config["log_dir"].as<std::string>();
            std::cout << "libOS log directory set to " << log_dir << std::endl;
        }
    } catch (YAML::ParserException& e) {
        std::cout << "Failed to parse config: " << e.what() << std::endl;
        exit(1);
    }

    /** Configure the network interface itself
     * (with as many rx/tx queue than we have service units using the device)
     */
    for (auto &d: devices_to_sus) {
        if (dmtr_net_port_init(d.first, net_ctx.net_mempool, d.second, d.second) != 0) {
            exit(1);
        }
    }

    /* Configure flow steering */
    for (auto &su: service_units) {
        if (dmtr_set_fdir(su.second->io_ctx.net_context) != 0) {
            exit(1);
        }
    }
}

/************** SERVICE UNITS ************/
int PspServiceUnit::socket(int &qd, int domain, int type, int protocol) {

    DMTR_OK(ioqapi.socket(qd, domain, type, protocol));
    DMTR_OK(ioqapi.set_io_ctx(qd, io_ctx.net_context));

    return 0;
}

#define WAIT_MAX_ITER 10000

int PspServiceUnit::wait(dmtr_qresult_t *qr_out, dmtr_qtoken_t qt) {
    int ret = EAGAIN;
    uint16_t iter = 0;
    while (EAGAIN == ret) {
        if (iter++ == WAIT_MAX_ITER) {
            return EAGAIN;
        }
        ret = ioqapi.poll(qr_out, qt);
    }
    DMTR_OK(ioqapi.drop(qt));
    return ret;
}

int PspServiceUnit::wait_any(dmtr_qresult_t *qr_out, int *start_offset, int *ready_offset, dmtr_qtoken_t qts[], int num_qts) {
    uint16_t iter = 0;
    while (1) {
        for (int i = start_offset? *start_offset : 0; i < num_qts; i++) {
            int ret = ioqapi.poll(qr_out, qts[i]);
            if (ret != EAGAIN) {
                if (ret == 0 || ret == ECONNABORTED || ret == ECONNRESET) {
                    DMTR_OK(ioqapi.drop(qts[i]));
                    if (ready_offset != NULL) {
                        *ready_offset = i;
                    }
                    if (start_offset != NULL && *start_offset != 0) {
                        *start_offset = 0;
                    }
                    return ret;
                }
            } else {
                if (iter++ == WAIT_MAX_ITER) {
                    if (start_offset != NULL) {
                        *start_offset = i;
                    }
                    return EAGAIN;
                }
            }
        }
        *start_offset = 0;
    }
    DMTR_UNREACHABLE();
}
