#include <string.h>
#include <stdio.h>
#include <stdint.h>
#include "httpops.hh"

int regex_html(char *to_match, char *htmlDoc, size_t length) {
    const char *pcreErrorStr;
    int errOffset;
    pcre *reCompiled = pcre_compile(EVIL_REGEX, 0, &pcreErrorStr, &errOffset, NULL);

    pcre_extra pcreExtra;
    pcreExtra.match_limit = -1;
    pcreExtra.match_limit_recursion = -1;
    pcreExtra.flags = PCRE_EXTRA_MATCH_LIMIT | PCRE_EXTRA_MATCH_LIMIT_RECURSION;

    int len = strlen(to_match);
    int x[1];

    int ret = pcre_exec(reCompiled, &pcreExtra, to_match, len, 0, 0, x, 1);

    char resp[5];
    if (ret < 0){
        sprintf(resp, "%s", "NO");
    } else {
        sprintf(resp, "%s", "YES");
    }
    snprintf(htmlDoc, length, HTML, to_match, EVIL_REGEX, resp);
    pcre_free(reCompiled);
    return 0;
}

void replace_special(char *url) {
    for (u_int16_t i=0; i<strlen(url); i++) {
        if (url[i] == '%') {
            char hex[3];
            hex[0] = url[i+1];
            hex[1] = url[i+2];
            hex[2] = '\0';
            char sub = (char)strtol(hex, NULL, 16);
            url[i] = sub;
            for (u_int16_t j=i+1; j<strlen(url) - 1; j++) {
                url[j] = url[j+2];
            }
        }
    }
}

int url_to_path(char *url, const char *dir, char *path, int capacity) {
    int len = 0;
    for (char *c=url; *c!='\0' && *c!='?'; c++, len++);

    int dir_len = strlen(dir);
    int dir_slash = (dir[dir_len - 1] == '/');
    int url_slash = (url[0] == '/');
    dir_len -= (dir_slash && url_slash);
    int both_missing_slash = (!dir_slash && !url_slash);

    if (dir_len + len + both_missing_slash >= capacity) {
        fprintf(stderr, "Path from url (%s) too large for buffer (%d)", url, capacity);
        return -1;
    }

    char *dest = path;

    strncpy(dest, dir, dir_len);
    if (both_missing_slash) {
        dest[dir_len] = '/';
    }
    strncpy(&dest[dir_len + both_missing_slash], url, len + 1);
    dest[dir_len + both_missing_slash + len] = '\0';
    replace_special(dest);
    return dir_len + len;
}

void path_to_mime_type(char *path, char buf[], int capacity) {
    char *extension = &path[strlen(path) - 1];
    for (; extension != path && *extension != '.' && *extension != '/'; extension--);

    if (*extension != '.')
        strncpy(buf, DEFAULT_MIME_TYPE, capacity);

    extension++; // Advance past the dot

    // TODO: Necessary to replace this by loading /etc/mime.types into hashmap?
    if (strcasecmp(extension, "html") == 0 ||
        strcasecmp(extension, "htm") == 0 ||
        strcasecmp(extension, "txt") == 0) {
        strncpy(buf, "text/html", capacity);
    } else if (strcasecmp(extension, "png") == 0) {
        strncpy(buf, "image/png", capacity);
    } else if (strcasecmp(extension, "jpg") == 0 ||
               strcasecmp(extension, "jpeg") == 0) {
        strncpy(buf, "image/jpeg", capacity);
    } else if (strcasecmp(extension, "gif") == 0) {
        strncpy(buf, "image/gif", capacity);
    } else {
        strncpy(buf, DEFAULT_MIME_TYPE, capacity);
    }
}

int generate_header(char **dest, int code, int body_len, char *mime_type) {
    if (code == 200) {
        char header_buf[MAX_HEADER_LEN];
        size_t alloc_size = snprintf(header_buf, 0, BASE_HTTP_HEADER, code, mime_type, body_len) + 1;
        *dest = reinterpret_cast<char *>(malloc(alloc_size));
        return snprintf(*dest, alloc_size, BASE_HTTP_HEADER, code, mime_type, body_len);
    } else if (code == 404) {
        size_t alloc_size = strlen(NOT_FOUND_HEADER) + 1;
        *dest = reinterpret_cast<char *>(malloc(alloc_size));
        return snprintf(*dest, alloc_size, NOT_FOUND_HEADER);
    } else if (code == 501) {
        size_t alloc_size = strlen(INT_ERROR_HEADER) + 1;
        *dest = reinterpret_cast<char *>(malloc(alloc_size));
        return snprintf(*dest, alloc_size, INT_ERROR_HEADER);
    } else {
        size_t alloc_size = strlen(NOT_IMPLEMENTED_HEADER) + 1;
        *dest = reinterpret_cast<char *>(malloc(alloc_size));
        return snprintf(*dest, alloc_size, NOT_IMPLEMENTED_HEADER);
    }
}

int get_regex_value(char *url, char *regex) {
    char *regex_start = strstr(url, REGEX_KEY);
    if (regex_start == NULL)
        return -1;
    regex_start += strlen(REGEX_KEY);
    int start_i = (regex_start - url);
    for (int i=start_i; (i-start_i)<MAX_REGEX_VALUE_LEN; i++) {
        switch (url[i]) {
            case '?':
            case '&':
            case '\0':
            case ' ':
                strncpy(regex, &url[start_i], i - start_i);
                regex[i-start_i] = '\0';
                return 0;
            default:
                continue;
        }
    }
    fprintf(stderr, "Requested regex value (%s) too long\n", regex_start);
    return -1;
}

enum http_req_type get_request_type(char *url) {
    if (strstr(url, REGEX_KEY) != NULL) {
        return REGEX_REQ;
    }
    return FILE_REQ;
}

void generate_response(char **response, char *header, char *body, int header_len,
                       int body_len, int *response_len, uint32_t req_id) {
    *response = reinterpret_cast<char *>(malloc(
        (size_t) body_len + header_len + 2 + sizeof(uint32_t)
    ));

    memcpy(*response, (const int *) &req_id, sizeof(uint32_t));
    *response_len = sizeof(uint32_t);

    *response_len += snprintf(*response + *response_len, header_len + 1, "%s", header);
    if (*response_len > (header_len + (int) sizeof(uint32_t))) {
        fprintf(stderr, "response_len > header_len: header was truncated!");
    }
    if (body) {
        *response_len += snprintf(*response + *response_len, body_len + 1, "%s", body);
        free(body);
    }
    if (*response_len > (body_len + header_len + (int) sizeof(uint32_t))) {
        fprintf(stderr, "response_len > header_len+body_len: body was truncated!");
    }
    free(header);
}
