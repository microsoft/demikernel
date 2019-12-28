#ifndef REQUEST_PARSER_H_
#define REQUEST_PARSER_H_

#include "http_parser.h"

#ifdef __cplusplus
extern "C" {
#endif

#define MAX_URL_SIZE 256
#define MAX_BODY_SIZE 1024 //128MB

#define MAX_FIELD_NAME_LEN 64
struct parser_state {
    char *url;
    size_t url_len;
    char *body;
    size_t body_len;
    size_t specified_body_len;
    char last_field_name[MAX_FIELD_NAME_LEN];
    int headers_complete;

    http_parser_settings settings;
    http_parser parser;
};

void init_parser_state(struct parser_state *state);
void clear_parser_state(struct parser_state *state);

enum parser_status {
    REQ_INCOMPLETE,
    REQ_COMPLETE,
    REQ_ERROR
};

enum parser_status parse_http(struct parser_state *state, char *buf, size_t bytes);

#ifdef __cplusplus
}
#endif

#endif
