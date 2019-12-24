#ifndef PSP_CL_CLIENT_H
#define PSP_CL_CLIENT_H

#include <pthread.h>
#include <netinet/in.h>
#include <arpa/inet.h>

#include <dmtr/libos.h>
#include <dmtr/libos/persephone.hh>

#include "../common/common.hh"
#include "../common/Request.hh"

class CLClientWorker : public PspWorker {
    private: boost::chrono::seconds duration_tp;
    private: std::vector<std::string> requests_str;
    private: uint16_t pipeline = 1;
    private: std::string remote_host;
    private: uint16_t remote_port;

    private: hr_clock::time_point start_time;
    private: int connfd = -1;
    private: uint32_t sent_requests = 0;
    private: uint32_t recv_requests = 0;
    public: std::unordered_map<uint32_t, std::unique_ptr<ClientRequest> > requests;

    public: CLClientWorker(int id, PspServiceUnit *su, int duration,
                           std::vector<std::string> requests, uint16_t ppl,
                           std::string rhost, uint16_t rport) :
                PspWorker(id, su), duration_tp(duration), requests_str(requests), pipeline(ppl),
                remote_host(rhost), remote_port(rport)
                {
                    requests.reserve(10000000); //XXX
                }
    public: ~CLClientWorker() {
               log_info("Sent: %d, Received: %d (missing: %d) ",
                        sent_requests, recv_requests, sent_requests - recv_requests);

            }
    private: int send_request(std::string request_str);
    private: int recv_request();
    private: int setup() {
                pin_thread(pthread_self(), 3+worker_id);
                //configure socket
                struct sockaddr_in saddr = {};
                saddr.sin_family = AF_INET;
                if (inet_pton(AF_INET, remote_host.c_str(), &saddr.sin_addr) != 1) {
                    log_error("Unable to parse host address: %s", strerror(errno));
                    return 1;
                }
                saddr.sin_port = htons(remote_port);
                log_info(
                    "Closed loop client worker set to send requests to %s:%d for %lu",
                    inet_ntoa(saddr.sin_addr), ntohs(saddr.sin_port), duration_tp.count()
                );
                DMTR_OK(psu->socket(connfd, AF_INET, SOCK_STREAM, 0));
                //connect
                DMTR_OK(psu->ioqapi.connect(
                    connfd, reinterpret_cast<struct sockaddr *>(&saddr), sizeof(saddr)
                ));
                start_time = take_time();
                //send pipeline requests
                for (int i = 0; i < pipeline - 1; ++i) {
                    std::string request_str = requests_str[sent_requests % requests_str.size()];
                    DMTR_OK(send_request(request_str));
                }
                return 0;
            }
    private: int dequeue(dmtr_qresult_t &dequeued) {
                if (take_time() - start_time > duration_tp) {
                    log_info("Receving pipelined requests (2s)...");
                    /* Now take 2 seconds to get pending requests */
                    duration_tp += boost::chrono::seconds(2);
                    for (uint16_t i = 0; (i < pipeline - 1) && (take_time() - start_time <= duration_tp); ++i) {
                        int wait_rtn = recv_request();
                        switch (wait_rtn) {
                            case ECONNABORTED:
                                log_info("ECONNABORTED while waiting for ppl req\n");
                                break;
                            case ECONNRESET:
                                log_info("ECONNRESET while waiting for ppl req\n");
                                break;
                            case ETIME:
                                log_info("ETIME while waiting for ppl req\n");
                                break;
                        }
                        if (wait_rtn == ECONNABORTED || wait_rtn == ECONNRESET || wait_rtn == ETIME) {
                            break;
                        }
                        DMTR_OK(wait_rtn);
                    }
                    terminate = true;
                    log_info(
                        "[W_%d] Sent: %d Rcvd: %d Missing: %d",
                        worker_id, sent_requests, recv_requests, sent_requests - recv_requests
                    );
                    psu->ioqapi.close(connfd);
                    return EAGAIN;
                }
                //Send request
                std::string request_str = requests_str[sent_requests % requests_str.size()];
                send_request(request_str);
                //Receive response
                int wait_rtn = recv_request();
                switch (wait_rtn) {
                    case ECONNABORTED:
                    case ECONNRESET:
                        log_info("Connection severed on worker %d", worker_id);
                        return EAGAIN;
                    case ETIME:
                        log_info("Time's up for worker %d", worker_id);
                        return EAGAIN;
                    case 0:
                    case EAGAIN:
                        return wait_rtn;
                    default:
                        DMTR_UNREACHABLE();
                }
            }
    private: int work(int status, dmtr_qresult_t &dequeued) {
                 DMTR_OK(status);
                return status;
            }
};
#endif //PSP_CL_CLIENT_H
