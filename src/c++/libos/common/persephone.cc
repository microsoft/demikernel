#include <iostream>

#include <yaml-cpp/yaml.h>
#include <dmtr/libos.h>
#include <dmtr/libos/persephone.hh>

#include <dmtr/annot.h>

/*
 * Enables the setup of global network context
 */
Psp::Psp(std::string &app_cfg) {
    /* Let network libOS init its specific EAL */
    dmtr_net_init(app_cfg.c_str());

    /* Allocate a mempool for the network */
    net_ctx.net_mempool = NULL;
    dmtr_net_mempool_init(&net_ctx.net_mempool, 0);

    /* Setup the fragmentation context */
    //TODO

    /* Parse the configuration */
    std::unordered_map<uint16_t, uint32_t> devices_to_sus;
    try {
        YAML::Node config = YAML::LoadFile(app_cfg);
        YAML::Node service_units = config["service_units"];
        for (size_t i = 0; i < service_units.size(); ++i) {
            for (auto su = service_units[i].begin(); su != service_units[i].end(); ++su) {
                auto key = su->first.as<std::string>();
                if (key == "io") {
                    for (size_t j = 0; j < su->second.size(); ++j) {
                        for (auto ioq = su->second[j].begin(); ioq != su->second[j].end(); ++ioq) {
                            auto ioq_key = ioq->first.as<std::string>();
                            if (ioq_key == "type" && ioq->second.as<std::string>() == "NETWORK_Q") {
                                auto dev_id = su->second[j]["device_id"].as<uint16_t>();
                                auto it = devices_to_sus.find(dev_id);
                                if (it == devices_to_sus.end()) {
                                    devices_to_sus.insert(
                                        std::pair<uint16_t, uint32_t>(dev_id, 1)
                                    );
                                } else {
                                    devices_to_sus[dev_id]++;
                                }
                            }
                        }
                    }
                }
            }
        }
    } catch (YAML::ParserException& e) {
        std::cout << "Failed to parse config: " << e.what() << std::endl;
        exit(1);
    }

    /** Configure the network interface itself
     * (with as many rx/tx queue than we have service units using the device)
     */
    for (auto &d: devices_to_sus) {
        dmtr_net_port_init(d.first, net_ctx.net_mempool, d.second, d.second);
    }
}
