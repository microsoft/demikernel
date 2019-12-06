#ifndef PERSEPHONE_H_
#define PERSEPHONE_H_

#include <yaml-cpp/yaml.h>

/*************** Gloval variables **************/
// Logging
static std::string log_dir;
#define MAX_LOG_FILENAME_LEN 128


/************** Control Plane class ************/
class Psp {
    public: Psp(std::string &app_cfg);

    private: struct net_context {
        void * net_mempool;
        void * frag_ctx;
    };

    private: struct net_context net_ctx;

};

#endif // PERSEPHONE_H_
