#ifndef PSP_HTTP_SERVER_H_
#define PSP_HTTP_SERVER_H_

#include "../common/common.hh"
#include "../common/PspWorker.hh"
#include "httpops.hh"

namespace po = boost::program_options;

class HttpWorker : public PspWorker {
    private: std::unordered_map<std::string, std::vector<char>> * const uri_store;
    public: HttpWorker(int id, PspServiceUnit *psu,
                       std::unordered_map<std::string, std::vector<char>> * const store)
                       : PspWorker(id, psu), uri_store(store) {}

    private: int setup() { return 0; }
    private: int dequeue(dmtr_qresult_t &dequeued) { return 0; }
    private: int work(int status, dmtr_qresult_t &result) { return 0; }
};

class NetWorker : public PspWorker {
    private: std::string dispatch_policy;
    public: NetWorker(int id, PspServiceUnit *psu, std::string &dp)
                       : PspWorker(id, psu), dispatch_policy(dp) {}

    private: int setup() { return 0; }
    private: int dequeue(dmtr_qresult_t &dequeued) { return 0; }
    private: int work(int status, dmtr_qresult_t &result) { return 0; }
};

class HttpServer {
    public: std::string uri_list;
    public: std::string dispatch_policy;
    public: std::unordered_map<std::string, std::vector<char>> uri_store;
    public: std::vector<NetWorker *> net_workers;
    public: std::vector<HttpWorker *> http_workers;
    public: Psp *psp;

    public: HttpServer(int argc, char *argv[]) {
                /* Retrieve config file from CLI */
                std::string cfg_file;
                po::options_description desc{"HTTP server options"};
                desc.add_options()
                    ("config-path,c", po::value<std::string>(&cfg_file)->required(), "path to configuration file");
                po::variables_map vm;
                try {
                    po::parsed_options parsed =
                        po::command_line_parser(argc, argv).options(desc).allow_unregistered().run();
                    po::store(parsed, vm);
                    if (vm.count("help")) {
                        std::cout << desc << std::endl;
                        exit(0);
                    }
                    notify(vm);
                } catch (const po::error &e) {
                    std::cerr << e.what() << std::endl;
                    std::cerr << desc << std::endl;
                    exit(0);
                }

                /* Extract all the configuration we need */
                try {
                    YAML::Node config = YAML::LoadFile(cfg_file);
                    if (!config["uri_list"].IsDefined()) {
                        PSP_WARN("No URI list provided");
                    } else {
                        uri_list = config["uri_list"].as<std::string>();
                    }
                    if (!config["dispatch_policy"].IsDefined()) {
                        PSP_WARN("No dispatch policy provided. Setting RR by default");
                        dispatch_policy = "RR";
                    } else {
                        dispatch_policy = config["dispatch_policy"].as<std::string>();
                    }
                } catch (YAML::ParserException& e) {
                    PSP_ERROR("Failed to parse config: " << e.what());
                    exit(1);
                }

                /* Init libOS */
                psp = new Psp(cfg_file);

                /* Pin main thread */
                pin_thread(pthread_self(), 0);

                /* Load URIs into the store */
                if (!uri_list.empty()) {
                    std::ifstream urifile(uri_list.c_str());
                    if (urifile.bad() || !urifile.is_open()) {
                        PSP_ERROR("Failed to open uri list file " << uri_list);
                    }
                    std::string uri;
                    while (std::getline(urifile, uri)) {
                        std::string full_uri = FILE_DIR + uri;
                        FILE *file = fopen(full_uri.c_str(), "rb");
                        if (file == NULL) {
                            PSP_ERROR(
                                "Failed to open '" << full_uri.c_str() << "': " << strerror(errno)
                            );
                        } else {
                            // Get file size
                            fseek(file, 0, SEEK_END);
                            int size = ftell(file);
                            if (size == -1) {
                                PSP_ERROR("could not ftell the file : " <<  strerror(errno));
                            }
                            fseek(file, 0, SEEK_SET);
                            std::vector<char> body(size);
                            size_t char_read = fread(&body[0], sizeof(char), size, file);
                            if (char_read < (unsigned) size) {
                                PSP_WARN(
                                    "fread() read less bytes than file's size (" << size << ")"
                                );
                            }
                            //std::cout << "Read " << char_read << " Bytes " << std::endl;
                            uri_store.insert(
                                std::pair<std::string, std::vector<char>>(full_uri, body)
                            );
                        }
                    }
                }
    }
};
#endif //PSP_HTTP_SERVER_H_
