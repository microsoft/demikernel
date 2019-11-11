#include <errno.h>
#include <signal.h>
#include <unistd.h>
#include <string.h>
#include <math.h>
#include <sys/types.h>
#include <sys/socket.h>
#include <sys/resource.h>
#include <arpa/inet.h>
#include <pthread.h>
#include <thread>
#include <mutex>
#include <chrono>
#include <ctime>
#include <queue>
#include <vector>
#include <unordered_map>
#include <fstream>

#include "app.hh"

#include <boost/optional.hpp>

#include <dmtr/annot.h>
#include <dmtr/fail.h>
#include <dmtr/latency.h>
#include <dmtr/time.hh>
#include <dmtr/libos.h>
#include <dmtr/wait.h>

//FIXME: new profiling has not been implemented for short lived mode.
//Need to use #LEGACY_PROFILING to get data

//FIXME: Now pending requests state are hashed based on their request ID
//short lived mode still does hash based on the operation token (which can result
//in incorrect tracing if the sender sends out of order).

//FIXME: short lived mode does not account for request_id being suffixed to the request

//FIXME:
// Multiple create/process/log thread sets are now disfunctionnal
// due to http_requests not being thread safe (TODO: create a
// vector of http_request, one per thread set

//FIXME: The whole logging thread should not be instantiated if none of the debug flag are set

/*****************************************************************
 *********************** GENERAL VARIABLES ***********************
 *****************************************************************/
bool long_lived = true;
bool terminate = false; /** This will be used when catching SIGINT */
bool check_resp_clen = false;

/*****************************************************************
 *********************** RATE VARIABLES **************************
 *****************************************************************/

/* Upper bound on request response time XXX */
int timeout_ns = 10000000;

/*****************************************************************
 *********************** CLIENT STRUCTS **************************
 *****************************************************************/
/* Each thread-group consists of a connect thread, a send/rcv thread, and a logging thread */
struct state_threads {
    std::thread *init;
    std::thread *resp;
    std::thread *log;
};
std::vector<state_threads *> threads;

/* Pre-formatted HTTP GET requests */
std::vector<std::unique_ptr<ClientRequest> > http_requests;

/*****************************************************************
 *********************** LOGGING *********************************
 *****************************************************************/
#ifdef OP_DEBUG
inline void print_op_debug(std::unordered_map<dmtr_qtoken_t, std::string> &m) {
    int net_pop = 0;
    int net_push = 0;
    for (auto &p: m) {
        if (p.second == "NET_POP") {
            net_pop++;
        } else if (p.second == "NET_PUSH") {
            net_push++;
        }
    }
    log_warn("%d NET_POP pending, %d NET_PUSH pending", net_pop, net_push);
}

std::vector<poll_q_len *> workers_pql;
#endif

/**
 * It is a bit dirty, but this function has two modes: live dump, or
 * post-mortem dump. In the former, request timing are directly dumped to their
 * respective file when recorded. In the latter, requests are recorded using dmtr_latency
 * objects, and dumped to files either when all requests in the experiment have been logged,
 * or when the thread times out.
 */
int log_responses(uint32_t total_requests, int log_memq,
                  hr_clock::time_point *time_end,
                  std::string log_dir, std::string label,
                  int my_idx) {

#ifdef LEGACY_PROFILING
    /* Init latencies */
    std::vector<struct log_data> logs;
    std::vector<const char *> latencies_names = {"end-to-end", "send", "receive"};
    if (long_lived) {
        latencies_names.push_back("connect");
    }

    for (auto &name: latencies_names) {
        struct log_data l;
        l.l = NULL; l.name = name; l.fh = NULL;
        strncpy(l.filename,
                generate_log_file_path(log_dir, label, name).c_str(),
                MAX_FILE_PATH_LEN
        );
        DMTR_OK(dmtr_new_latency(&l.l, name));
        logs.push_back(l);
    }
#endif
#ifdef DMTR_TRACE
    FILE *f = fopen(generate_log_file_path(log_dir, label, "traces").c_str(), "w");
    if (f) {
        fprintf(f, "REQ_ID\tSENDING\tREADING\tCOMPLETED\tPUSH_TOKEN\tPOP_TOKEN\n");
    } else {
        log_error("Could not open log file!!");
    }
#endif

    uint32_t n_invalid = 0;
    bool expired = false;
    uint32_t logged = 0;
    dmtr_qresult_t wait_out;
    dmtr_qtoken_t token;
    DMTR_OK(dmtr_pop(&token, log_memq));
    while (logged < total_requests && !terminate) {
        int status = dmtr_wait(&wait_out, token);
        if (status == 0) {
            ClientRequest *req = reinterpret_cast<ClientRequest *>(
                wait_out.qr_value.sga.sga_segs[0].sgaseg_buf
            );

#ifdef DMTR_TRACE
            if (f) {
                fprintf(
                    f, "%d\t%lu\t%lu\t%lu\t%lu\t%lu\n",
                    req->id,
                    since_epoch(req->sending),
                    since_epoch(req->reading),
                    since_epoch(req->completed),
                    req->push_token,
                    req->pop_token
                );
            }
#endif

#ifdef LEGACY_PROFILING //FIXME: check if file handler is NULL
            if (long_lived) {
                DMTR_OK(dmtr_record_timed_latency(logs[0].l, since_epoch(req->sending),
                                                  ns_diff(req->sending, req->completed)));
            } else {
                DMTR_OK(dmtr_record_timed_latency(logs[0].l, since_epoch(req->connecting),
                                                  ns_diff(req->connecting, req->completed)));
                DMTR_OK(dmtr_record_timed_latency(logs[3].l, since_epoch(req->connecting),
                                                  ns_diff(req->connecting, req->connected)));
            }
            DMTR_OK(dmtr_record_timed_latency(logs[1].l, since_epoch(req->sending),
                                              ns_diff(req->sending, req->reading)));
            DMTR_OK(dmtr_record_timed_latency(logs[2].l, since_epoch(req->reading),
                                              ns_diff(req->reading, req->completed)));
#endif
            logged++;
            if (!req->valid) {
                n_invalid++;
            }
            delete req;
            DMTR_OK(dmtr_pop(&token, log_memq));
        } else {
            if (status != EAGAIN) {
                log_warn("Logger's %d memory queue returned: %d", my_idx, status);
            }
        }

        if (take_time() > *time_end) {
            log_warn("logging time has passed. %d requests were logged (%d invalid).",
                      logged, n_invalid);
            expired = true;
            break;
        }
    }

#ifdef LEGACY_PROFILING
    dump_logs(logs, log_dir, label);
#endif
#ifdef OP_DEBUG
    dump_pql(workers_pql[my_idx], log_dir, label);
#endif
    if (f) {
        fclose(f);
    }

    if (!expired) {
        log_info("Log thread %d exiting after having logged %d requests (%d invalid).",
                my_idx, logged, n_invalid);
    }
    dmtr_close(log_memq);
    return 0;
}

/*****************************************************************
 ****************** PROCESS CONNECTIONS **************************
 *****************************************************************/

int process_connections(int my_idx, uint32_t total_requests, hr_clock::time_point *time_end,
                        int process_conn_memq, int log_memq) {
    int start_offset = 0;
    bool expired = false;
    uint32_t completed = 0;
    uint32_t dequeued = 0;
    std::vector<dmtr_qtoken_t> tokens;
    tokens.reserve(total_requests);
    dmtr_qtoken_t token = 0;
    std::unordered_map<int, ClientRequest *> requests;
    requests.reserve(total_requests);
    dmtr_pop(&token, process_conn_memq);
    tokens.push_back(token);
    //std::vector<std::pair<dmtr_qtoken_t, std::string> > token_to_op;
    //token_to_op.push_back(std::pair<dmtr_qtoken_t, std::string>(token, "MEMQ_POP"));
    while (completed < total_requests) {
        if (tokens.empty()) {
            if (take_time() > *time_end) {
                expired = true;
                log_warn("process time has passed. %d requests were processed.", completed);
            /*
                printf("=====================\n");
                printf("%lu Pending operations\n", token_to_op.size());
                for (auto &t: token_to_op) {
                    printf("%ld -> %s; ", t.first, t.second.c_str());
                }
                printf("\n=====================\n");
            */
                break;
            }
            continue;
        }

        /* Now wait_any and process pop/push task results */
        dmtr_qresult_t wait_out;
        int idx;
#ifdef DMTR_TRACE
        hr_clock::time_point op_time = take_time();
#endif
        int status = dmtr_wait_any(&wait_out, &start_offset, &idx, tokens.data(), tokens.size());
        tokens.erase(tokens.begin()+idx);
        if (status == 0) {
            /*
            token_to_op.erase(token_to_op.begin() + idx);
            printf("=====================\n");
            printf("%ld -> %s operated\n", tokens[idx], dmtr_opcode_labels[wait_out.qr_opcode]);
            printf("=====================\n");
            printf("Pending operations\n");
            for (auto &t: token_to_op) {
                printf("%ld -> %s; ", t.first, t.second.c_str());
            }
            printf("\n=====================\n");
            */
#ifdef OP_DEBUG
            update_pql(tokens.size(), workers_pql[my_idx]);
#endif
            /* Is this a new connection is ready to be processed ? */
            if (wait_out.qr_qd == process_conn_memq) {
                ClientRequest *request = reinterpret_cast<ClientRequest *>(
                    wait_out.qr_value.sga.sga_segs[0].sgaseg_buf
                );
                dmtr_sgarray_t sga;
                sga.sga_numsegs = 1;
                sga.sga_segs[0].sgaseg_len = request->req_size;
                sga.sga_segs[0].sgaseg_buf = (void *) request->req;
                dequeued++;

#if defined(DMTR_TRACE) || defined(LEGACY_PROFILING)
                update_request_state(*request, SENDING, op_time);
#endif
                DMTR_OK(dmtr_push(&token, request->conn_qd, &sga));
                tokens.push_back(token);
                requests.insert(std::pair<int, ClientRequest *>(request->conn_qd, request));
                //token_to_op.push_back(std::pair<dmtr_qtoken_t, std::string>(token, "CONN_PUSH"));

                /* Re-enable memory queue for reading */
                DMTR_OK(dmtr_pop(&token, process_conn_memq));
                tokens.push_back(token);
                //token_to_op.push_back(std::pair<dmtr_qtoken_t, std::string>(token, "MEMQ_POP"));

                /*
                printf("=====================\n");
                printf("Pending operations\n");
                for (auto &t: token_to_op) {
                    printf("%ld -> %s; ", t.first, t.second.c_str());
                }
                printf("\n=====================\n");
                */

                continue;
            }

            auto req = requests.find(wait_out.qr_qd);
            if (req == requests.end()) {
                log_error("OP'ed on an unknown request qd?");
                exit(1);
            }
            ClientRequest *request = req->second;
            if (wait_out.qr_opcode == DMTR_OPC_PUSH) {
                /* Create pop task now that data was sent */
                DMTR_OK(dmtr_pop(&token, wait_out.qr_qd));
                tokens.push_back(token);
                /*
                token_to_op.push_back(std::pair<dmtr_qtoken_t, std::string>(token, "CONN_POP"));
                printf("=====================\n");
                printf("Pending operations\n");
                for (auto &t: token_to_op) {
                    printf("%ld -> %s; ", t.first, t.second.c_str());
                }
                printf("\n=====================\n");
                */

#if defined(DMTR_TRACE) || defined(LEGACY_PROFILING)
                update_request_state(*request, READING, op_time);
#endif
            } else if (wait_out.qr_opcode == DMTR_OPC_POP) {
                assert(wait_out.qr_value.sga.sga_numsegs== 1);
                /* Log and complete request now that we have the answer */
#if defined(DMTR_TRACE) || defined(LEGACY_PROFILING)
                update_request_state(*request, COMPLETED, op_time);
#endif
                /* Null terminate the response */
                char *req_c = reinterpret_cast<char *>(
                    wait_out.qr_value.sga.sga_segs[0].sgaseg_buf
                );

                std::string resp_str(req_c);
                request->valid = validate_response(resp_str, check_resp_clen);
                free(wait_out.qr_value.sga.sga_segs[0].sgaseg_buf);
                DMTR_OK(dmtr_close(wait_out.qr_qd));
                requests.erase(req);

                dmtr_sgarray_t sga;
                sga.sga_numsegs = 1;
                sga.sga_segs[0].sgaseg_len = sizeof(ClientRequest);
                sga.sga_segs[0].sgaseg_buf = reinterpret_cast<void *>(request);
                dmtr_push(&token, log_memq, &sga);
                while(dmtr_wait(NULL, token) == EAGAIN) {
                    if (terminate) {
                        return 0;
                    }
                    continue;
                }

                completed++;
            } else {
                log_warn("Non supported OP code");
            }
        } else {
            if (status == EAGAIN) {
                if (terminate) {
                    log_info(" worker %d set to terminate", my_idx);
                    break;
                }
                continue;
            }
            log_warn("Got status %d out of dmtr_wait_any", status);
            assert(status == ECONNRESET || status == ECONNABORTED);
            dmtr_close(wait_out.qr_qd);
        }

        if (take_time() > *time_end) {
            log_warn("process time has passed. %d requests were processed.", completed);
            expired = true;
            /*
            printf("=====================\n");
            printf("%lu Pending operations\n", token_to_op.size());
            for (auto &t: token_to_op) {
                printf("%ld -> %s; ", t.first, t.second.c_str());
            }
            printf("\n=====================\n");
            */
            break;
        }
    }

    if (!expired) {
        log_info("Process thread %d exiting after having processed %d requests", my_idx, completed);
    }
    return 0;
}

/*****************************************************************
 ****************** CREATE I/O QUEUES ****************************
 *****************************************************************/

int create_queues(double interval_ns, int n_requests, std::string host, int port,
                  int process_conn_memq, hr_clock::time_point *time_end, int my_idx) {
    /* Configure target */
    struct sockaddr_in saddr = {};
    saddr.sin_family = AF_INET;
    if (inet_pton(AF_INET, host.c_str(), &saddr.sin_addr) != 1) {
        log_error("Unable to parse IP address!: %s", strerror(errno));
        return -1;
    }
    saddr.sin_addr.s_addr = htonl(ntohl(saddr.sin_addr.s_addr) + my_idx * 2);
    saddr.sin_port = htons(port);
    log_info("Thread %d sending requests to %s", my_idx, inet_ntoa(saddr.sin_addr));

    // We use std chrono here, rather than Boost's, because it's more convenient for sleep_until
    std::chrono::steady_clock::time_point create_start_time = std::chrono::steady_clock::now();

    /* Times at which the connections should be initiated */
    std::chrono::steady_clock::time_point send_times[n_requests];

    for (int i = 0; i < n_requests; ++i) {
        std::chrono::nanoseconds elapsed_time((long int)(interval_ns * i));
        send_times[i] = create_start_time + elapsed_time;
    }

    int connected = 0;
    bool expired = false;
    for (int interval = 0; interval < n_requests; interval++) {
        /* Wait until the appropriate time to create the connection */
        std::this_thread::sleep_until(send_times[interval]);

        std::unique_ptr<ClientRequest> req = std::move(http_requests[interval]);

        /* Create Demeter queue */
        int qd = 0;
        DMTR_OK(dmtr_socket(&qd, AF_INET, SOCK_STREAM, 0));
        req->conn_qd = qd;

        /* Connect */
#if defined(DMTR_TRACE) || defined(LEGACY_PROFILING)
        update_request_state(*req, CONNECTING, take_time());
#endif
        DMTR_OK(dmtr_connect(qd, reinterpret_cast<struct sockaddr *>(&saddr), sizeof(saddr)));
        connected++;
#if defined(DMTR_TRACE) || defined(LEGACY_PROFILING)
        update_request_state(*req, CONNECTED, take_time());
#endif

        /* Make this request available to the request handler thread */
        dmtr_sgarray_t sga;
        sga.sga_numsegs = 1;
        //FIXME the process_connection() thread should now make this ptr unique at dequeue.
        sga.sga_segs[0].sgaseg_buf = reinterpret_cast<void *>(req.release());
        sga.sga_segs[0].sgaseg_len = sizeof(req);

        dmtr_qtoken_t token;
        dmtr_push(&token, process_conn_memq, &sga);
        while(dmtr_wait(NULL, token) == EAGAIN) {
            if (terminate) {
                break;
            }
            continue;
        }

        if (take_time() > *time_end) {
            log_warn("create time has passed. %d connection were established.", connected);
            expired = true;
            break;
        }
    }

    if (!expired) {
        log_info("Create thread %d exiting after having created %d requests", my_idx, connected);
    }
    return 0;
}

/**********************************************************************
 ********************** LONG LIVED CONNECTION MODE ********************
 **********************************************************************/
int long_lived_processing(double interval_ns, uint32_t n_requests, std::string host, int port,
                          int log_memq, hr_clock::time_point *time_end, int my_idx,
                          bool debug_duration_flag) {
    /* Configure target */
    struct sockaddr_in saddr = {};
    saddr.sin_family = AF_INET;
    if (inet_pton(AF_INET, host.c_str(), &saddr.sin_addr) != 1) {
        log_error("Unable to parse IP address!: %s", strerror(errno));
        return -1;
    }
    saddr.sin_addr.s_addr = htonl(ntohl(saddr.sin_addr.s_addr) + my_idx * 2);
    saddr.sin_port = htons(port);
    log_info("Thread %d sending requests to %s", my_idx, inet_ntoa(saddr.sin_addr));

    hr_clock::time_point create_start_time = take_time();

    /* Times at which the requests should be initiated */
    hr_clock::time_point send_times[n_requests];

    for (uint32_t i = 0; i < n_requests; ++i) {
        boost::chrono::nanoseconds elapsed_time((long int)(interval_ns * i));
        send_times[i] = create_start_time + elapsed_time;
    }

    /* Create Demeter queue */
    int qd = 0;
    DMTR_OK(dmtr_socket(&qd, AF_INET, SOCK_STREAM, 0));
    /* Connect */
    DMTR_OK(dmtr_connect(qd, reinterpret_cast<struct sockaddr *>(&saddr), sizeof(saddr)));

    int start_offset = 0;
    bool expired = false;
    uint32_t completed = 0;
    uint32_t send_index = 0;
    dmtr_qtoken_t token;
    std::vector<dmtr_qtoken_t> tokens;
    tokens.reserve(1024); // If we go beyond we really have other performance problems anyway
    std::unordered_map<uint32_t, std::unique_ptr<ClientRequest> > requests;
    requests.reserve(1024);
#ifdef OP_DEBUG
    std::unordered_map<dmtr_qtoken_t, std::string> pending_ops;
#endif
    /* If we still have pending requests, or we have pending push to logger's memory queue */
    while (completed < n_requests || tokens.size() > 0) {
        if (terminate) {
            log_warn(" worker %d set to terminate", my_idx);
            break;
        }
        hr_clock::time_point maintenant = take_time();
        if (maintenant > *time_end && !debug_duration_flag) {
            log_warn("Process time has passed. %d requests were processed.", completed);
            expired = true;
#ifdef OP_DEBUG
            int missing = n_requests - completed;
            if (missing > 0) {
                print_op_debug(pending_ops);
            }
#endif
            break;
        }
        /** First check if it is time to emmit a new request over the connection */
        if (maintenant > send_times[send_index] && send_index < n_requests) {
            std::unique_ptr<ClientRequest> request = std::move(http_requests[send_index]);
            //printf("Retrieving request %d stored in unique ptr %p (%p) from http_requests\n", request->id, &request, request.get());
            assert(send_index == request->id);
            send_index++;
            dmtr_sgarray_t sga;
            sga.sga_numsegs = 1;
            sga.sga_segs[0].sgaseg_len = request->req_size;
            sga.sga_segs[0].sgaseg_buf = (void *) request->req;
#if defined(DMTR_TRACE) || defined(LEGACY_PROFILING)
            update_request_state(*request, SENDING, take_time());
#endif
            DMTR_OK(dmtr_push(&token, qd, &sga));
            tokens.push_back(token);
            log_debug("Scheduling request %d for send", request->id);
            //printf("Sending %s\n", request->req+sizeof(uint32_t));
            //printf("Scheduled PUSH: %d/%lu\n", request->id, token);
            requests.insert(std::pair<uint32_t, std::unique_ptr<ClientRequest> >(request->id, std::move(request)));
#ifdef OP_DEBUG
            pending_ops.insert(std::pair<dmtr_qtoken_t, std::string>(token, "NET_PUSH"));
#endif
        }

        if (tokens.empty()) {
            continue;
        }

        dmtr_qresult_t wait_out;
        int idx;
        int status = dmtr_wait_any(&wait_out, &start_offset, &idx, tokens.data(), tokens.size());
#ifdef DMTR_TRACE
        hr_clock::time_point op_time = take_time();
#endif
        if (status == EAGAIN) {
            continue;
        }
        token = tokens[idx];
        tokens.erase(tokens.begin()+idx);
        if (status == 0) {
#ifdef OP_DEBUG
            update_pql(tokens.size(), workers_pql[my_idx]);
#endif
            assert(wait_out.qr_value.sga.sga_numsegs == 1);

            if (wait_out.qr_qd == log_memq) {
                assert(wait_out.qr_opcode == DMTR_OPC_PUSH);
                continue;
            }

            /* Retrieve the ID before the request is processed further */
            uint32_t * const req_id_ptr =
                reinterpret_cast<uint32_t *>(wait_out.qr_value.sga.sga_segs[0].sgaseg_buf);
            uint32_t req_id = *req_id_ptr;
            /*
            if (wait_out.qr_opcode == DMTR_OPC_PUSH) {
                printf("PUSHED %d/%lu\n", req_id, token);
            } else if (wait_out.qr_opcode == DMTR_OPC_POP) {
                printf("POPED %d/%lu\n", req_id, token);
            }
            */
            /*
            char *rf = r+sizeof(uint32_t);
            printf("Response str: %s (%p)\n", rf, rf);
            */

            auto req = requests.find(req_id);
            if (req == requests.end()) {
                log_error("[OPC %d] operated unregistered request %d (%p) (completed/sent so far: %d/%d)",
                           wait_out.qr_opcode,
                           req_id, wait_out.qr_value.sga.sga_segs[0].sgaseg_buf,
                           completed, send_index);
                //terminate = true;
                continue;
            }
            std::unique_ptr<ClientRequest> request = std::move(req->second);
            //printf("Found request %p (%p) mapped to req id %d\n", &request, request.get(), req_id);
            assert(request->id == req_id);
            //printf("There are %zu items in the requests map\n", requests.size());

            if (wait_out.qr_opcode == DMTR_OPC_PUSH) {
#ifdef OP_DEBUG
                pending_ops.erase(token);
#endif
#if defined(DMTR_TRACE) || defined(LEGACY_PROFILING)
                update_request_state(*request, READING, op_time);
                request->push_token = token;
#endif
                log_debug("Scheduling request %d for read", request->id);
                DMTR_OK(dmtr_pop(&token, wait_out.qr_qd));
                tokens.push_back(token);

                free(request->req); //XXX putting this in the log threads causes ASAN heap-read-after-free??
                //printf("Scheduled POP %d/%lu\n", request->id, token);
                requests[request->id] = std::move(request);
#ifdef OP_DEBUG
                pending_ops.insert(std::pair<dmtr_qtoken_t, std::string>(token, "NET_POP"));
#endif
            } else if (wait_out.qr_opcode == DMTR_OPC_POP) {
#ifdef OP_DEBUG
                pending_ops.erase(token);
#endif
#if defined(DMTR_TRACE) || defined(LEGACY_PROFILING)
                update_request_state(*request, COMPLETED, op_time);
                request->pop_token = token;
#endif
                /* Null terminate the response (for validate_response) */
                char *req_c = reinterpret_cast<char *>(
                    wait_out.qr_value.sga.sga_segs[0].sgaseg_buf
                );
                req_c[wait_out.qr_value.sga.sga_segs[0].sgaseg_len - 1] = '\0';
                std::string resp_str(req_c+sizeof(uint32_t));
                request->valid = validate_response(resp_str, check_resp_clen);
                free(wait_out.qr_value.sga.sga_segs[0].sgaseg_buf);

                ClientRequest *ptrtoreq = request.release();
                requests.erase(ptrtoreq->id);
                //printf("Retired %d\n", ptrtoreq->id);
                log_debug("Retired request %d", ptrtoreq->id);
                completed++;

                dmtr_sgarray_t sga;
                sga.sga_numsegs = 1;
                sga.sga_segs[0].sgaseg_len = sizeof(request);
                sga.sga_segs[0].sgaseg_buf = reinterpret_cast<void *>(ptrtoreq);
                dmtr_push(&token, log_memq, &sga);
                tokens.push_back(token);
            }
        } else {
            assert(status == ECONNRESET || status == ECONNABORTED);
            DMTR_OK(dmtr_close(wait_out.qr_qd));
        }

        if (take_time() > *time_end && !debug_duration_flag) {
            log_warn("Process time has passed. %d requests were processed.", completed);
            expired = true;
#ifdef OP_DEBUG
            int missing = n_requests - completed;
            if (missing > 0) {
                print_op_debug(pending_ops);
            }
#endif
            break;
        }
    }

    if (!expired) {
        log_info(
            "Long lived process thread %d exiting after having created %d and processed %d requests",
            my_idx, send_index, completed
        );
    }

    return 0;
}

void sig_handler(int signo) {
    printf("Entering signal handler\n");
    terminate = true;
    printf("Exiting signal handler\n");
}

void pin_thread(pthread_t thread, u_int16_t cpu) {
    cpu_set_t cpuset;
    CPU_ZERO(&cpuset);
    CPU_SET(cpu, &cpuset);

    int rtn = pthread_setaffinity_np(thread, sizeof(cpu_set_t), &cpuset);
    if (rtn != 0) {
        fprintf(stderr, "could not pin thread: %s\n", strerror(errno));
    }
}

/* TODO
 * - Handle maximum concurrency?
 * - Add TID to logs
 */

/**
 * FIXME:
 * - Split IP/address and URI from the url argument (use server_ip_address from yaml config)
 */
int main(int argc, char **argv) {
    int rate, duration, n_threads;
    std::string url, uri_list, label, log_dir;
    namespace po = boost::program_options;
    bool short_lived, debug_duration_flag, flag_type;
    po::options_description desc{"Rate client options"};
    desc.add_options()
        ("debug-duration", po::bool_switch(&debug_duration_flag), "Remove duration limits for threads")
        ("short-lived", po::bool_switch(&short_lived), "Re-use connection for each request")
        ("rate,r", po::value<int>(&rate)->required(), "Start rate")
        ("duration,d", po::value<int>(&duration)->required(), "Duration")
        ("url,u", po::value<std::string>(&url)->required(), "Target URL")
        ("label,l", po::value<std::string>(&label)->default_value("rate_client"), "experiment label")
        ("log-dir,L", po::value<std::string>(&log_dir)->default_value("./"), "Log directory")
        ("client-threads,T", po::value<int>(&n_threads)->default_value(1), "Number of client threads")
        ("uri-list,f", po::value<std::string>(&uri_list)->default_value(""), "List of URIs to request")
        ("flag-type", po::bool_switch(&flag_type), "Specify request type in User-Agent");
    parse_args(argc, argv, false, desc);

    if (short_lived) {
        log_info("Starting client in short lived mode");
        long_lived = false;
    }

#ifdef OP_DEBUG
    workers_pql.reserve(PQL_RESA);
#endif

    static const size_t host_idx = url.find_first_of("/");
    if (host_idx == std::string::npos) {
        log_error("Wrong URL format given (%s)", url.c_str());
        return -1;
    }
    std::string uri = url.substr(host_idx+1);
    std::string host = url.substr(0, host_idx);

    if (rate < n_threads ) {
        n_threads = rate;
        log_warn("Adjusting number of threads to match rate (now %d)", n_threads);
    }

    /* Compute request per interval per thread */
    // First, distribute requests among threads:
    double rate_per_thread = (double) rate / n_threads;

    // Compute interval -- the time we have to send a request -- per thread
    double interval_ns_per_thread = 1000000000.0 / rate_per_thread;

    uint32_t total_requests = rate * duration;
    uint32_t req_per_thread = total_requests / n_threads;

    /* Setup Demeter */
    DMTR_OK(dmtr_init(argc , argv));
    //sleep(3);

    /* Setup worker threads */
    int nw;
    if (short_lived) {
        nw = 3;
    } else {
        nw = 2;
    }

    log_info("Starting %d*%d threads to serve %d requests (%d reqs / thread)",
              n_threads, nw, total_requests, req_per_thread);
    log_info("Requests per second: %d. Adjusted total requests: %d", rate, req_per_thread*n_threads);
    log_info("Interval size: %.2f ns", interval_ns_per_thread);

    /* Pre-compute the HTTP requests */
    if (!uri_list.empty()) {
        /* Loop-over URI file to create requests */
        std::ifstream urifile(uri_list.c_str());
        if (urifile.bad() || !urifile.is_open()) {
            log_error("Failed to open uri list file");
            return -1;
        }
        /* We loop in case the URI file was not created with the desired amount of requests in mind */
        while (http_requests.size() < total_requests) {
            while (std::getline(urifile, uri)) {
                std::string user_agent;
                std::string req_uri;
                if (flag_type) {
                    std::stringstream ss(uri);
                    getline(ss, user_agent, ',');
                    if (user_agent.size() == uri.size()) {
                        log_error("flag-type requested but request %s has no flag", uri.c_str());
                        exit(1);
                    }
                    getline(ss, req_uri, ',');
                } else {
                    user_agent = DEFAULT_USER_AGENT;
                    req_uri = uri;
                }

                char * const req = reinterpret_cast<char *>(malloc(MAX_REQUEST_SIZE));
                memset(req, '\0', MAX_REQUEST_SIZE);
                uint32_t id = (uint32_t) http_requests.size();
                memcpy(req, (uint32_t *) &id, sizeof(uint32_t));
                size_t req_size = snprintf(
                    req + sizeof(uint32_t), MAX_REQUEST_SIZE - sizeof(uint32_t),
                    REQ_STR, req_uri.c_str(), host.c_str(), user_agent.c_str()
                );
                req_size += sizeof(uint32_t);
                std::unique_ptr<ClientRequest> req_obj(new ClientRequest(req, req_size, id));
                http_requests.push_back(std::move(req_obj));
            }
            urifile.clear(); //Is this needed in c++11?
            urifile.seekg(0, std::ios::beg);
        }
    } else {
        /* All requests are the one given to the CLI */
        for (uint32_t i = 0; i < total_requests; ++i) {
            char * const req = reinterpret_cast<char *>(malloc(MAX_REQUEST_SIZE));
            memset(req, '\0', MAX_REQUEST_SIZE);
            memcpy(req, (uint32_t *) &i, sizeof(uint32_t));
            size_t req_size = snprintf(
                req + sizeof(uint32_t), MAX_REQUEST_SIZE - sizeof(uint32_t),
                REQ_STR, uri.c_str(), host.c_str(), DEFAULT_USER_AGENT.c_str()
            );
            req_size += sizeof(uint32_t);
            std::unique_ptr<ClientRequest> req_obj(new ClientRequest(req, req_size, i));
            http_requests.push_back(std::move(req_obj));
        }
    }

    /* Determine max thread running time */
    // Give one extra second to initiate request
    boost::chrono::seconds duration_init(duration + 1);
    hr_clock::time_point time_end_init = take_time() + duration_init;

    // Give five extra second to send and gather responses
    boost::chrono::seconds duration_process(duration + 5);
    hr_clock::time_point time_end_process = take_time() + duration_process;

    // Give ten extra seconds to log responses
    boost::chrono::seconds duration_log(duration + 15);
    hr_clock::time_point time_end_log = take_time() + duration_log;

    /* Block SIGINT to ensure handler will only be run in main thread */
    sigset_t mask, oldmask;
    sigemptyset(&mask);
    sigaddset(&mask, SIGINT);
    sigaddset(&mask, SIGQUIT);
    int ret = pthread_sigmask(SIG_BLOCK, &mask, &oldmask);
    if (ret != 0) {
        fprintf(stderr, "Couln't block SIGINT and SIGQUIT: %s\n", strerror(errno));
    }
    if (signal(SIGINT, sig_handler) == SIG_ERR)
        std::cout << "\ncan't catch SIGINT\n";
    if (signal(SIGTERM, sig_handler) == SIG_ERR)
        std::cout << "\ncan't catch SIGTERM\n";

    /* Initialize responses first, then logging, then request initialization */
    for (int i = 0; i < n_threads; ++i) {
        struct state_threads *st = new state_threads();
        int log_memq;
        DMTR_OK(dmtr_queue(&log_memq));
        st->log = new std::thread(
            log_responses,
            req_per_thread, log_memq, &time_end_log,
            log_dir, label, i
        );
        pin_thread(st->log->native_handle(), i+8);
        if (long_lived) {
            /** some book-keeping */
            st->resp = new std::thread(
                long_lived_processing,
                interval_ns_per_thread, req_per_thread,
                host, port, log_memq, &time_end_process,
                i, debug_duration_flag
            );
            pin_thread(st->resp->native_handle(), i+9);
#ifdef OP_DEBUG
            workers_pql.push_back(new poll_q_len());
#endif
        } else {
            int process_conn_memq;
            DMTR_OK(dmtr_queue(&process_conn_memq));
            st->resp = new std::thread(
                process_connections,
                i, req_per_thread, &time_end_process,
                process_conn_memq, log_memq
            );
            pin_thread(st->init->native_handle(), i+4);
            st->init = new std::thread(
                create_queues,
                interval_ns_per_thread, req_per_thread,
                host, port, process_conn_memq, &time_end_init, i
            );
            pin_thread(st->init->native_handle(), i+5);
        }
        threads.push_back(st);
    }

    /* Re-enable SIGINT and SIGQUIT */
    ret = pthread_sigmask(SIG_SETMASK, &oldmask, NULL);
    if (ret != 0) {
        fprintf(stderr, "Couln't block SIGINT: %s\n", strerror(errno));
    }

    // Wait on all of the initialization threads
    if (short_lived) {
        for (int i = 0; i < n_threads; i++) {
            threads[i]->init->join();
            delete threads[i]->init;
        }
        log_info("Requests creation finished");
    }

    // Wait on the response threads
    // TODO: In short lived mode, when init threads stop early, signal this to stop
    for (int i = 0; i < n_threads; i++) {
        threads[i]->resp->join();
        delete threads[i]->resp;
    }
    log_info("Responses gathered");

    // Wait on the logging threads TODO also shut this off if previous threads stop early
    for (int i = 0; i < n_threads; i++) {
        threads[i]->log->join();
        delete threads[i]->log;
    }

    for (int i = 0; i < n_threads; i++) {
        delete threads[i];
#ifdef OP_DEBUG
        delete workers_pql[i];
#endif
    }

    return 0;
}
