# Demeter

## The Demete C-API


```
    int dmtr_init(int argc, char *argv[]);
```
TBD

```
    int dmtr_queue(int *qd_out);
```
TBD

```
    int dmtr_socket(int *qd_out, int domain, int type, int protocol);
```
TBD

```
    int dmtr_getsockname(int qd, struct sockaddr *saddr, socklen_t *size);
```
TBD

```
    int dmtr_listen(int fd, int backlog);
```
TBD

```
    int dmtr_bind(int qd, const struct sockaddr *saddr, socklen_t size);
```
TBD

```
    int dmtr_accept(dmtr_qtoken_t *qtok_out, int sockqd);
```
TBD

```
    int dmtr_connect(int qd, const struct sockaddr *saddr, socklen_t size);
```
TBD

```
    int dmtr_close(int qd);
```
TBD

```
    int dmtr_is_qd_valid(int *flag_out, int qd);
```
TBD

```
    int dmtr_push(dmtr_qtoken_t *qtok_out, int qd, const dmtr_sgarray_t *sga);
```
TBD

```
    int dmtr_pop(dmtr_qtoken_t *qt_out, int qd);
```
TBD

```
    int dmtr_poll(dmtr_qresult_t *qr_out, dmtr_qtoken_t qt);
```
TBD

```
    int dmtr_drop(dmtr_qtoken_t qt);
```
TBD
