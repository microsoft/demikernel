#ifndef PSP_REQUEST_H
#define PSP_REQUEST_H

#include "common.hh"

class Request {
    public: Request(const uint32_t id) : id(id) {};
    public: uint32_t id;
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
    public: char * req; /** The actual request */
    public: size_t req_size; /** Number of Bytes in the request */
    public: int conn_qd; /** The connection's queue descriptor */

    public: static ClientRequest* format_request(uint32_t id, std::string &req_uri, std::string &host);

    public: ClientRequest(char * const req, size_t req_size, uint32_t id) :
                Request(id), req(req), req_size(req_size) {}
};

#endif // PSP_REQUEST_H
