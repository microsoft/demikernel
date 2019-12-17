#include "Request.hh"
#include <fstream>

ClientRequest * ClientRequest::format_request(uint32_t id, std::string &req_uri, std::string &host) {
    /* Extract request type from request string */
    std::string req_type;
    std::string uri;
    std::stringstream ss(req_uri);
    getline(ss, req_type, ',');
    if (req_type.size() == req_uri.size()) {
        log_error("Request type not present in URI!");
        exit(1);
    }
    getline(ss, uri, ',');

    const char *REQ_STR =
        "GET /%s HTTP/1.1\r\nHost: %s\r\nConnection: close\r\nUser-Agent: %s\r\n\r\n";

    /* Allocate and format buffer */
    char * const req = static_cast<char *>(malloc(MAX_REQUEST_SIZE));
    memset(req, '\0', MAX_REQUEST_SIZE);
    /* Prepend request ID to payload */
    memcpy(req, (uint32_t *) &id, sizeof(uint32_t));
    size_t req_size = snprintf(
        req + sizeof(uint32_t), MAX_REQUEST_SIZE - sizeof(uint32_t),
        REQ_STR, uri.c_str(), host.c_str(), req_type.c_str()
    );
    req_size += sizeof(uint32_t);
    return new ClientRequest(req, req_size, id);
}
