#ifndef PSP_REQUEST_H
#define PSP_REQUEST_H

#include "common.hh"

enum req_type {
    UNKNOWN = 0,
    ALL,
    REGEX,
    PAGE,
    POPULAR_PAGE,
    UNPOPULAR_PAGE,
};

class Request {
    public: uint32_t id;
    public: char * req; /** The actual request */
    public: size_t req_size; /** Number of Bytes in the request */
    public: uint32_t conn_qd; /** Origin connection's queue descriptor */
    public: Request(const uint32_t id, char * req, size_t rsize) :
            id(id), req(req), req_size(rsize) {};
    public: Request(const uint32_t qfd) : conn_qd(qfd){};
};

#define MAX_REQUEST_SIZE 4192

class ClientRequest : public Request {
#ifdef TRACE
    public: hr_clock::time_point connecting;
    public: hr_clock::time_point connected;
    public: hr_clock::time_point sending;
    public: hr_clock::time_point reading;
    public: hr_clock::time_point completed;
    public: dmtr_qtoken_t push_token;
    public: dmtr_qtoken_t pop_token;
#endif
    public: bool valid; /** Whether the response was valid */

    public: static ClientRequest* format_request(uint32_t id, std::string &req_uri, std::string &host);

    /*
    public: ClientRequest(char * const req, size_t req_size, uint32_t id) :
                Request(id, req, req_size) {}
    */
    using Request::Request;
};


class ServerRequest : public Request {
#ifdef TRACE
    public: dmtr_qtoken_t pop_token;
    public: dmtr_qtoken_t push_token;
    public: hr_clock::time_point net_receive;
    public: hr_clock::time_point http_dispatch;
    public: hr_clock::time_point start_http;
    public: hr_clock::time_point end_http;
    public: hr_clock::time_point http_done;
    public: hr_clock::time_point net_send;
#endif
    public: enum req_type type;
    public: dmtr_sgarray_t sga;
    public: char *data; /** Pointer to the first sga segment buffer */
    public: size_t data_len; /** buffer len */
    public: ServerRequest(uint32_t qfd, dmtr_sgarray_t &sga):
                Request(qfd), sga(sga),
                data(static_cast<char *>(sga.sga_segs[0].sgaseg_buf)),
                data_len(static_cast<size_t>(sga.sga_segs[0].sgaseg_len)) {}
};

#endif // PSP_REQUEST_H
