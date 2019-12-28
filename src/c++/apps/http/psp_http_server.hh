#ifndef PSP_HTTP_SERVER_H_
#define PSP_HTTP_SERVER_H_

//#include <netinet/in.h>
#include <arpa/inet.h>
#include <sys/stat.h>
#include "../common/common.hh"
#include "../common/PspWorker.hh"
#include "../common/Request.hh"
#include "httpops.hh"
#include "request_parser.h"

#define MAX_CLIENTS 64

namespace po = boost::program_options;

enum http_type { NET, HTTP };

class HttpWorker : public PspWorker {
    public: std::unordered_map<std::string, std::vector<char>> *uri_store;
    public: HttpWorker(int id, PspServiceUnit *psu,
                       std::unordered_map<std::string, std::vector<char>> * const store)
                       : PspWorker(id, psu), uri_store(store) {}

    private: static inline void regex_work(char *url, char **response, int *response_len, uint32_t req_id) {
                 char *body = NULL;
                 int body_len = 0;
                 int code = 200;
                 char mime_type[MAX_MIME_TYPE];
                 char regex_value[MAX_REGEX_VALUE_LEN];
                 int rtn = get_regex_value(url, regex_value);
                 if (rtn != 0) {
                     fprintf(stderr, "Non-regex URL passed to craft_regex_response!\n");
                     code = 501;
                 } else {
                     char html[8192];
                     rtn = regex_html(regex_value, html, 8192);
                     if (rtn < 0) {
                         fprintf(stderr, "Error crafting regex response\n");
                         code = 501;
                     }
                     body_len = strlen(html);
                     body = reinterpret_cast<char *>(malloc(body_len+1));
                     snprintf(body, body_len+1, "%s", html);
                     strncpy(mime_type, "text/html", MAX_MIME_TYPE);
                 }

                 char *header = NULL;
                 int header_len = generate_header(&header, code, body_len, mime_type);
                 generate_response(response, header, body, header_len, body_len, response_len, req_id);
            }

    private: inline void file_work(char *url, char **response, int *response_len, uint32_t req_id) {
                 char filepath[MAX_FILEPATH_LEN];
                 url_to_path(url, FILE_DIR, filepath, MAX_FILEPATH_LEN);
                 struct stat st;
                 int status = stat(filepath, &st);

                 char *body = NULL;
                 int body_len = 0;
                 int code = 404;
                 char mime_type[MAX_MIME_TYPE];
                 if (status != 0 || S_ISDIR(st.st_mode)) {
                     if (status != 0) {
                         log_warn("Failed to get status of requested file %s\n", filepath);
                     } else {
                         log_warn("Directory requested (%s). Returning 404.\n", filepath);
                     }
                     strncpy(mime_type, "text/html", MAX_MIME_TYPE);
                 } else {
                     if (uri_store->empty()) {
                         PSP_WARN("URI not found");
                     } else {
                         auto it = uri_store->find(std::string(filepath));
                         if (it == uri_store->end()) {
                             log_error("Requested non registered file (%s)", filepath);
                         } else {
                             /* We copy bytes here to simulate transfering them from memory to client */
                             std::vector<char> data(it->second);
                         }
                     }
                 }
                 //FIXME: move on to full answer generation (segmentation is fixed)
                 body = NULL;
                 body_len = 0;
                 char *header = NULL;
                 int header_len = generate_header(&header, code, body_len, mime_type);
                 generate_response(response, header, body, header_len, body_len, response_len, req_id);
             }

    public: enum req_type my_req_type;
    private: struct parser_state *state;
    private: std::vector<dmtr_qtoken_t> tokens;
    private: int setup() {
                 assert(peer_ids.size() > 0);
                 pin_thread(pthread_self(), 4 + peer_ids.size() + worker_id);

                 /* Setup HTTP parser */
                 state = static_cast<parser_state *>(malloc(sizeof(*state)));
                 state->url = NULL;
                 state->body = NULL;

                 /* Schedule a pop on all the networkers IO queues */
                 dmtr_qtoken_t token;
                 for (auto &p: peer_ids) {
                     DMTR_OK(psu->ioqapi.pop(token, get_peer_qd(p)));
                 }

                 return 0;
             }

    private: int start_offset = 0;
    private: int dequeue(dmtr_qresult_t &dequeued) {
                 int idx;
                 int status = psu->wait_any(
                     &dequeued, &start_offset, &idx, tokens.data(), tokens.size()
                 );
                 if (status == EAGAIN) {
                     return EAGAIN;
                 }
                 DMTR_OK(status);
                 tokens.erase(tokens.begin() + idx);
                 dmtr_qtoken_t token;
                 DMTR_OK(psu->ioqapi.pop(token, dequeued.qr_qd));
                 tokens.push_back(token);
                 return status;
             }

    private: int work(int status, dmtr_qresult_t &dequeued) {
                 /* Grab the request */
                 ServerRequest *req = reinterpret_cast<ServerRequest *>(
                    dequeued.qr_value.sga.sga_segs[0].sgaseg_buf
                 );
                 uint32_t * const req_id = reinterpret_cast<uint32_t *>(req->data);
                 req->id = *req_id;
                 req->req  = req->data + sizeof(uint32_t);
                 PSP_DEBUG("HTTP worker poped req " << req->id << ": " << req->req);
                 req->req_size = req->data_len - sizeof(uint32_t);

                 /* Parse HTTP content */
                 clear_parser_state(state); //FIXME: do we even need this?
                 enum parser_status pstatus = parse_http(state, req->req, req->req_size);
                 switch (pstatus) {
                     case REQ_COMPLETE:
                        //fprintf(stdout, "HTTP worker got complete request\n");
                         break;
                     case REQ_INCOMPLETE:
                     case REQ_ERROR:
                         log_warn("HTTP worker got incomplete or malformed request: %.*s",
                                  (int) req->req_size, req->req);
                         clear_parser_state(state); //FIXME: do we even need this?

                         /* Free original request's sga */
                         dmtr_free_mbuf(&req->sga);

                         /* Craft error response */
                         req->data = static_cast<char *>(
                             malloc(strlen(BAD_REQUEST_HEADER) + 1 + sizeof(uint32_t))
                         );
                         memcpy(req->data, (uint32_t *) &req->id, sizeof(uint32_t));
                         req->sga.sga_segs[0].sgaseg_len = snprintf(
                             req->data + sizeof(uint32_t),
                             strlen(BAD_REQUEST_HEADER) + 1, "%s", BAD_REQUEST_HEADER
                         ) + sizeof(uint32_t);

                         /* Push back the ServerRequest to the net worker */
                         if (blocking_push_to_peer(dequeued.qr_qd, dequeued.qr_value.sga) == -1) {
                             log_warn("Could not push to net worker on %d", dequeued.qr_qd);
                         } else {
                             log_debug("NetWorker pushed to peer on %d", dequeued.qr_qd);
                         }
                         return -1;
                 }

                 /* Handle the request */
                 char *response = NULL;
                 int response_size;
                 switch(get_request_type(state->url)) {
                     case REGEX_REQ:
                         regex_work(state->url, &response, &response_size, req->id);
                         break;
                     case FILE_REQ:
                         file_work(state->url, &response, &response_size, req->id);
                         break;
                 }

                 if (response == NULL) {
                     log_error("Error formatting HTTP response");
                     return -1;
                 }

                 /* Craft the response */
                 dmtr_free_mbuf(&req->sga);
                 req->data = response;
                 req->sga.sga_segs[0].sgaseg_len = response_size;
                 /* Push back the ServerRequest to the net worker */
                 if (blocking_push_to_peer(dequeued.qr_qd, dequeued.qr_value.sga) == -1) {
                     log_warn("Could not push to net worker on %d", dequeued.qr_qd);
                 } else {
                     log_debug("NetWorker pushed to peer on %d", dequeued.qr_qd);
                 }

                 return 0;
             }
};

class NetWorker : public PspWorker {
    public: NetWorker(int id, PspServiceUnit *psu, std::string &dp)
                       : PspWorker(id, psu), dispatch_policy(dp) {}

    /* Scheduling */
    public: std::vector<HttpWorker*> regex_workers;
    public: std::vector<HttpWorker*> file_workers;
    private: std::string dispatch_policy;
    private: std::function<enum req_type(std::string &)> net_dispatch_f;
    private: static inline enum req_type psp_get_req_type(std::string &url) {
                 //get a pointer to the User-Agent part of the string
                 size_t user_agent_pos = url.find("User-Agent:");
                 if (user_agent_pos == std::string::npos) {
                     log_error("User-Agent was not present in request headers!");
                     return UNKNOWN;
                 }
                 user_agent_pos += 12; // "User-Agent: "
                 size_t body_pos = url.find("\r\n\r\n");
                 if (body_pos == std::string::npos) {
                     log_error("User-Agent was not present in request headers!");
                     return UNKNOWN;
                 }
                 std::string req_type(url.substr(user_agent_pos, body_pos));
                 if (strncmp(req_type.c_str(), "REGEX", 5) == 0) {
                     return REGEX;
                 } else if (strncmp(req_type.c_str(), "PAGE", 4) == 0) {
                     return PAGE;
                 } else {
                     return UNKNOWN;
                 }
             }

    /* Work */
    private: std::vector<bool> clients_in_waiting;
    private: int num_rcvd = 0;
    private: std::unordered_map<enum req_type, uint64_t> type_counts;
    private: int lqd;
    private: int start_offset = 0;
    private: struct sockaddr_in saddr;
    private: std::vector<dmtr_qtoken_t> tokens;
    private: int setup() {
                 pin_thread(pthread_self(), 4 + worker_id);

                 /* Set dispatch function */
                 net_dispatch_f = psp_get_req_type;

                 /* Use the SU address:port to bind() */
                 DMTR_NOTNULL(EINVAL, psu);
                 saddr.sin_family = AF_INET;
                 DMTR_OK(!inet_aton(psu->ip.c_str(), &saddr.sin_addr));
                 saddr.sin_port = htons(psu->port);
                 DMTR_OK(psu->socket(lqd, AF_INET, SOCK_STREAM, 0));
                 if (psu->ioqapi.bind(lqd,
                                      reinterpret_cast<struct sockaddr *>(&saddr),
                                      sizeof(saddr))) {
                     PSP_ERROR("Binding failed: " << strerror(errno));
                     exit(1);
                 }
                 DMTR_OK(psu->ioqapi.listen(lqd, 100)); //XXX
                 dmtr_qtoken_t token;
                 DMTR_OK(psu->ioqapi.accept(token, lqd));
                 tokens.push_back(token);

                 /* we should schedule a pop only if we scheduled a write
                 for (int peer_id : peer_ids) {
                     DMTR_OK(pop_from_peer(peer_id, token));
                     tokens.push_back(token);
                 }
                 */

                 clients_in_waiting.reserve(MAX_CLIENTS);
                 return 0;
             }

    private: int dequeue(dmtr_qresult_t &dequeued) {
                int idx;
                int status = psu->wait_any(
                    &dequeued, &start_offset, &idx, tokens.data(), tokens.size()
                );
                if (status == EAGAIN) {
                    return EAGAIN;
                }
                tokens.erase(tokens.begin() + idx);
                //PSP_DEBUG("wait_any returned " << status);
                if (status == ECONNABORTED || status == ECONNRESET) {
                    uint64_t qd = dequeued.qr_qd;
                    PSP_DEBUG(
                        "Removing closed connection " << qd << " from answerable list"
                    );
                    if (clients_in_waiting[qd]) {
                        tokens.erase(
                            std::remove_if(
                                tokens.begin(), tokens.end(),
                                [qd](dmtr_qtoken_t &t) -> bool { return (t >> 32) == qd; }
                            ),
                            tokens.end()
                        );
                    }
                    PSP_INFO("closing pseudo connection on " << qd);
                    psu->ioqapi.close(qd);
                    return EAGAIN;
                }
                return status;
             }

    private: int choose_worker(ServerRequest &req) {
                 if (dispatch_policy == "RR") {
                     return num_rcvd % peer_ids.size();
                 } else if (dispatch_policy == "FILTER") {
                     /* Retrieve request type */
                     std::string req_str(req.data + sizeof(uint32_t));
                     req.type = net_dispatch_f(req_str);
                     /* Apply a round robin policy across worker types  */
                     switch (req.type) {
                         default:
                             PSP_ERROR("Unknown request type");
                             break;
                         case REGEX:
                             return type_counts[REGEX]++ % regex_workers.size();
                         case PAGE:
                             return type_counts[PAGE]++ % file_workers.size();
                     }
                 } else {
                     PSP_ERROR("Unknown dispatching policy " << dispatch_policy);
                     exit(1);
                 }
                 return -1;
             }

    private: int work(int status, dmtr_qresult_t &dequeued) {
                 dmtr_qtoken_t token;
                 /* Handle messages on accept queue */
                 if (dequeued.qr_qd == lqd) {
                     assert(DMTR_OPC_ACCEPT == dequeued.qr_opcode);
                     DMTR_OK(psu->ioqapi.pop(token, dequeued.qr_value.ares.qd));
                     tokens.push_back(token);
                     DMTR_OK(psu->ioqapi.accept(token, lqd));
                     tokens.push_back(token);
                 }

                 int dequeued_id = get_peer_id(dequeued.qr_qd);
                 /* Handle notification that we pushed to worker */
                 if (dequeued.qr_opcode == DMTR_OPC_PUSH) {
                     if (int peer_id = get_peer_qd(peer_id) == -1) {
                         /* This must be a client worker */
                         free(dequeued.qr_value.sga.sga_segs[0].sgaseg_buf);
                         dequeued.qr_value.sga.sga_segs[0].sgaseg_buf = NULL;
                     } else {
                         /* This must be an HTTP worker, so we schedule a pop on it */
                         DMTR_OK(psu->ioqapi.pop(token, dequeued.qr_qd));
                         tokens.push_back(token);
                         return 0;
                     }
                 }

                 assert(DMTR_OPC_POP == dequeued.qr_opcode);
                 if (dequeued_id == -1) {
                     //FIXME make this a unique ptr?
                     ServerRequest *req = new ServerRequest(dequeued.qr_qd, dequeued.qr_value.sga);
                     int peer_id = choose_worker(*req);
                     int qd = get_peer_qd(peer_id);
                     if (qd == -1) {
                         PSP_ERROR("Invalid HTTP worker selected");
                         exit(1);
                     }
                     dmtr_sgarray_t sga_req;
                     as_sga(*req, sga_req);
                     DMTR_OK(psu->ioqapi.push(token, qd, sga_req));
                     tokens.push_back(token);
                     DMTR_OK(psu->ioqapi.pop(token, dequeued.qr_qd));
                     tokens.push_back(token);
                     clients_in_waiting[dequeued.qr_qd] = true;
                 } else {
                     ServerRequest *req = reinterpret_cast<ServerRequest *>(
                         dequeued.qr_value.sga.sga_segs[0].sgaseg_buf
                     );
                     /* Check whether the connection is still active */
                     if (clients_in_waiting[req->conn_qd] == false) {
                         PSP_WARN("Dropping obsolete message aimed towards closed connection");
                         free(req->data);
                         req->data = NULL;
                         return 0;
                     }
                     DMTR_OK(psu->ioqapi.push(token, req->conn_qd, req->sga));
                     tokens.push_back(token);
                 }

                 return 0;
             }
};

class HttpServer {
    public: std::string uri_list;
    public: std::string dispatch_policy;
    public: std::unordered_map<std::string, std::vector<char>> uri_store;
    public: std::vector<NetWorker *> net_workers;
    public: std::vector<HttpWorker *> http_workers;
    public: std::vector<HttpWorker *> file_workers;
    public: std::vector<HttpWorker *> regex_workers;
    public: std::unordered_map<enum req_type, uint16_t> typed_workers;
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
                    /* Setup the typed workers */
                    if (config["n_regex_workers"].IsDefined()) {
                        typed_workers[REGEX] = config["n_regex_workers"].as<uint16_t>();
                    }
                    if (config["n_file_workers"].IsDefined()) {
                        typed_workers[PAGE] = config["n_file_workers"].as<uint16_t>();
                    }
                } catch (YAML::ParserException& e) {
                    PSP_ERROR("Failed to parse config: " << e.what());
                    exit(1);
                }

                /* Init libOS */
                psp = new Psp(cfg_file);
                size_t n_typed_workers = typed_workers[PAGE] + typed_workers[REGEX];
                if (psp->service_units.size() < n_typed_workers) {
                    PSP_ERROR("Not enough service units to accomodate number of typed workers");
                    exit(1);
                }

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
