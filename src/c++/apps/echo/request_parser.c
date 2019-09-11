#include "request_parser.h"
#include "string.h"
#include "stdio.h"
#include "stdlib.h"

#ifdef __GNUC__
#define UNUSED __attribute__ ((unused))
#else
#define UNUSED
#endif

static int url_callback(http_parser *parser, const char *at, size_t length) {
    struct parser_state *state = parser->data;
    strncpy(&state->url[state->url_len], at, length);
    state->url[state->url_len + length] = '\0';
    state->url_len += length;
    //printf("Got URL: %s\n", state->url);
    return 0;
}

static int headers_complete_callback(http_parser *parser) {
    struct parser_state *state = parser->data;
    state->headers_complete = 1;
    //printf("Got end of headers\n");
    return 0;
}

static int header_field_callback(http_parser *parser,
                                 const char *at,
                                 size_t length) {
    struct parser_state *state = parser->data;
    size_t trunc_length = length < MAX_FIELD_NAME_LEN ? length : MAX_FIELD_NAME_LEN;
    strncpy(state->last_field_name, at, trunc_length);
    state->last_field_name[trunc_length] = '\0';
    //printf("Got header field: %s\n", state->last_field_name);
    return 0;
}

static int header_value_callback(http_parser *parser,
                                 const char *at,
                                 size_t length) {
    struct parser_state *state = parser->data;
    if (strcmp(state->last_field_name, "Content-Length") == 0) {
        char tmp[length + 1];
        strncpy(tmp, at, length);
        tmp[length] = '\0';
        state->specified_body_len = atoi(tmp);
        //printf("Got header value: %s\n", tmp);
    }
    return 0;
}

static int body_callback(http_parser *parser,
                         const char *at,
                         size_t length) {
    struct parser_state *state = parser->data;
    strncpy(&state->body[state->body_len], at, length);
    state->body[state->body_len + length] = '\0';
    state->body_len += length;
    //printf("Got body: %s\n", state->body);
    return 0;
}

void init_parser_state(struct parser_state *state) {
    memset(state, 0, sizeof(*state));
    state->settings.on_url = url_callback;
    state->settings.on_header_field = header_field_callback;
    state->settings.on_header_value = header_value_callback;
    state->settings.on_headers_complete = headers_complete_callback;
    state->settings.on_body = body_callback;
    state->url = (char *) malloc(MAX_URL_SIZE);
    state->body = (char *) malloc(MAX_BODY_SIZE);

    http_parser_init(&state->parser, HTTP_REQUEST);
    state->parser.data = (void*)state;
}

enum parser_status parse_http(struct parser_state *state, char *buf, size_t bytes) {
    if (state == NULL) {
        printf("Cannot handle connection with NULL state\n");
        return REQ_ERROR;
    }

    if (bytes > MAX_URL_SIZE) {
        printf("Request too large\n");
        return REQ_ERROR;
    }

    if (state->headers_complete) {
        printf("Parsing even though header is already complete\n");
    }

    //printf("Attempting to parse '%.*s'\n", (int)bytes, buf);
    size_t nparsed = http_parser_execute(&state->parser, &state->settings, buf, bytes);
    if (nparsed != bytes) {
        buf[bytes] = '\0';
        printf("Error parsing HTTP request '%s'\n(%s:%s) (should have parsed %zd more bytes)\n",
               buf,
               http_errno_name(state->parser.http_errno),
               http_errno_description(state->parser.http_errno),
               bytes - nparsed
        );
        return REQ_ERROR;
    }
    if (state->headers_complete && state->specified_body_len <= state->body_len) {
        return REQ_COMPLETE;
    } else {
        return REQ_INCOMPLETE;
    }
}
